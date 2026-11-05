"""Thumbnail - a home page of thumbnails for the videos on your drives.

Runs a small local server (standard library only) and opens the UI in an
app-mode browser window. The portable folder looks like this:

    Thumbnail.exe        starts runtime\\pythonw.exe app\\app.py
    app\\                this file and web\\ (the page)
    runtime\\            a minimal Python
    data\\               library, covers, channel photos, collections, settings

    python app.py --no-window   start only the server
"""
import base64
import http.client
import json
import os
import re
import subprocess
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

# abspath, not resolve(): keep the drive letter as Windows shows it (resolve() turns
# mapped and subst drives into their real path, which breaks portable paths).
APP_DIR = Path(os.path.abspath(__file__)).parent
ROOT_DIR = APP_DIR.parent                # the portable folder
WEB_DIR = APP_DIR / "web"
DATA_DIR = ROOT_DIR / "data"
COVERS_DIR = DATA_DIR / "covers"
AVATARS_DIR = DATA_DIR / "avatars"
LIBRARY_FILE = DATA_DIR / "library.json"
CHANNELS_FILE = DATA_DIR / "channels.json"
COLLECTIONS_FILE = DATA_DIR / "collections.json"
SETTINGS_FILE = DATA_DIR / "settings.json"       # theme and thumbnail size
PLAYLIST_FILE = DATA_DIR / "play-all.m3u"
# The browser keeps its own files (caches etc.) on each PC, outside the portable folder.
BROWSER_PROFILE = Path(os.environ.get("LOCALAPPDATA") or os.environ.get("TEMP") or str(APP_DIR)) / "Thumbnail" / "browser"

HOST, PORT = "127.0.0.1", 8765
ORIGIN = f"http://{HOST}:{PORT}"

VIDEO_EXTS = {".mp4", ".mkv", ".avi", ".mov", ".wmv", ".webm", ".m4v", ".flv",
              ".mpg", ".mpeg", ".ts", ".m2ts", ".3gp", ".ogv", ".vob"}
IMAGE_EXTS = {"image/jpeg": ".jpg", "image/png": ".png", "image/webp": ".webp",
              "image/gif": ".gif", "image/bmp": ".bmp", "image/avif": ".avif"}
MEDIA_TYPES = {".mp4": "video/mp4", ".m4v": "video/mp4", ".webm": "video/webm", ".mov": "video/quicktime",
               ".mkv": "video/x-matroska", ".ts": "video/mp2t", ".m2ts": "video/mp2t", ".flv": "video/x-flv",
               ".wmv": "video/x-ms-wmv", ".avi": "video/x-msvideo", ".mpg": "video/mpeg", ".mpeg": "video/mpeg",
               ".ogv": "video/ogg", ".3gp": "video/3gpp", ".vob": "video/mpeg"}
# Fixed list instead of the mimetypes module (which scans the Windows registry at startup).
FILE_TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
              ".css": "text/css; charset=utf-8", ".woff2": "font/woff2", ".svg": "image/svg+xml",
              **{ext: mime for mime, ext in IMAGE_EXTS.items()}}

# The window pings every few seconds; once it is closed the server exits.
IDLE_TIMEOUT = 150   # long enough to survive timer throttling of a minimized window
CLOSE_GRACE = 8      # after the page says goodbye (allows for a reload)


# --------------------------------------------------------------------------- library

def load_json(path, default):
    if path.exists():
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            path.with_suffix(f".broken-{int(time.time())}.json").write_bytes(path.read_bytes())
    return default


def write_json(path, data):
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
    os.replace(tmp, path)


def write_image(data_url, folder, stem):
    """Saves a base64 data URL into folder and returns the new file name."""
    m = re.match(r"data:([\w/+.-]+);base64,(.*)", data_url or "", re.S)
    if not m or m.group(1) not in IMAGE_EXTS:
        raise ValueError("Unsupported image")
    name = f"{stem}-{int(time.time() * 1000)}{IMAGE_EXTS[m.group(1)]}"
    (folder / name).write_bytes(base64.b64decode(m.group(2)))
    return name


def unlink_quietly(path):
    try:
        path.unlink()
    except OSError:
        pass


def folder_of(item):
    p = Path(item["path"])
    return p.parent.name or str(p.anchor)


def channel_of(item):
    # A video belongs to its folder's "channel" until you name one.
    return item.get("channel") or folder_of(item)


# ---- portable paths: the app folder can live on an external drive whose letter changes

def to_disk_path(path):
    """Videos on the app's own drive are saved relative to the app folder (e.g. ..\\Videos\\x.mp4)."""
    if os.path.splitdrive(path)[0].lower() == os.path.splitdrive(str(ROOT_DIR))[0].lower():
        try:
            return os.path.relpath(path, ROOT_DIR)
        except ValueError:
            pass
    return path


def from_disk_path(path):
    return path if os.path.isabs(path) else os.path.normpath(os.path.join(ROOT_DIR, path))


def find_on_other_drive(path):
    """The same file under another drive letter (a drive that came back as G: instead of F:)."""
    drive, rest = os.path.splitdrive(path)
    if len(drive) != 2 or drive[1] != ":":
        return None
    try:
        drives = os.listdrives()
    except AttributeError:
        drives = [f"{c}:\\" for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ" if os.path.exists(f"{c}:\\")]
    for root in drives:
        candidate = root[:2] + rest
        if root[:2].lower() != drive.lower() and os.path.isfile(candidate):
            return candidate
    return None


class Library:
    def __init__(self, path, channels_path, collections_path):
        self.path = path
        self.channels_path = channels_path
        self.collections_path = collections_path
        self.lock = threading.RLock()
        self.items = load_json(path, [])
        for i in self.items:
            i["path"] = from_disk_path(i["path"])  # in memory, paths are always absolute
        self.channels = load_json(channels_path, {})  # name -> {"avatar": file}
        # Playlists: [{"id", "name", "videos": [ids in order], "created", "updated"}]
        self.collections = load_json(collections_path, [])

    def prune_files(self):
        """Deletes covers and photos nothing points to any more, and leftover temp files."""
        with self.lock:
            used = ({i.get("cover") for i in self.items} | {c.get("cover") for c in self.collections}
                    | {ch.get("avatar") for ch in self.channels.values()})
            for folder in (COVERS_DIR, AVATARS_DIR):
                for f in folder.iterdir():
                    if f.is_file() and f.name not in used:
                        unlink_quietly(f)
            for name in os.listdir(DATA_DIR):
                if name.endswith(".tmp"):
                    unlink_quietly(DATA_DIR / name)

    def reconnect_drives(self):
        """Missing videos that turn up under another drive letter are pointed there. Returns how many."""
        moved = 0
        with self.lock:
            for i in self.items:
                if not os.path.isfile(i["path"]):
                    found = find_on_other_drive(i["path"])
                    if found:
                        i["path"], moved = found, moved + 1
            if moved:
                self.save()
        return moved

    def save(self):
        with self.lock:
            write_json(self.path, [dict(i, path=to_disk_path(i["path"])) for i in self.items])

    def save_channels(self):
        with self.lock:
            write_json(self.channels_path, self.channels)

    def save_collections(self):
        with self.lock:
            write_json(self.collections_path, self.collections)

    # ---- collections

    def collection(self, cid):
        with self.lock:
            return next((c for c in self.collections if c["id"] == cid), None)

    def create_collection(self, name, ids=()):
        with self.lock:
            now = time.time()
            col = {"id": uuid.uuid4().hex[:10], "name": name, "videos": [], "created": now, "updated": now}
            self.collections.append(col)
            self.collect(col, ids)  # also saves
            return col

    def collect(self, col, ids):
        """Appends videos to a collection, skipping ones already in it."""
        with self.lock:
            known = {i["id"] for i in self.items}
            for vid in ids:
                if vid in known and vid not in col["videos"]:
                    col["videos"].append(vid)
            col["updated"] = time.time()
            self.save_collections()

    def uncollect(self, col, ids):
        with self.lock:
            col["videos"] = [v for v in col["videos"] if v not in set(ids)]
            col["updated"] = time.time()
            self.save_collections()

    def ids_for(self, paths):
        """Library ids of these files (new or already added), in the given order."""
        with self.lock:
            by_path = {os.path.normcase(os.path.abspath(i["path"])): i["id"] for i in self.items}
            return [by_path[k] for k in (os.path.normcase(os.path.abspath(p)) for p in paths) if k in by_path]

    def get(self, item_id):
        with self.lock:
            return next((i for i in self.items if i["id"] == item_id), None)

    def add_paths(self, paths):
        added = []
        with self.lock:
            known = {os.path.normcase(os.path.abspath(i["path"])) for i in self.items}
            for p in paths:
                p = os.path.abspath(p)
                key = os.path.normcase(p)
                if key in known or not os.path.isfile(p):
                    continue
                known.add(key)
                item = {
                    "id": uuid.uuid4().hex[:12],
                    "path": p,
                    "title": Path(p).stem,
                    "cover": None,
                    "coverAuto": False,
                    "autoTried": False,
                    "duration": None,
                    "added": time.time(),
                    "lastPlayed": None,
                }
                self.items.append(item)
                added.append(item["id"])
            if added:
                self.save()
        return added

    def public(self):
        out = []
        with self.lock:
            for i in self.items:
                try:  # one disk query per video (this runs after every change)
                    size = os.stat(i["path"]).st_size
                except OSError:
                    size = None
                channel = channel_of(i)
                out.append({
                    **i,
                    "exists": size is not None,
                    "size": size,
                    "folder": folder_of(i),
                    "channel": channel,
                    "channelAvatar": self.channels.get(channel, {}).get("avatar"),
                    "ext": Path(i["path"]).suffix.lstrip(".").upper(),
                })
        return out

    def set_channel(self, items, name, photo=None, apply_to_all=False):
        """Moves the videos (or every video of a single video's channel) to channel
        `name`. photo: None keeps the photo, "remove" clears it, or a data URL."""
        with self.lock:
            olds = {channel_of(i) for i in items}
            targets = [i for i in self.items if channel_of(i) in olds] if apply_to_all else items
            for i in targets:
                i["channel"] = name
            # Renaming a whole channel takes its photo along.
            old = next(iter(olds)) if len(olds) == 1 else None
            if apply_to_all and old and name != old and old in self.channels and name not in self.channels:
                self.channels[name] = self.channels.pop(old)
            if photo:
                current = self.channels.pop(name, {}).get("avatar")
                if current:
                    unlink_quietly(AVATARS_DIR / current)
                if photo != "remove":
                    self.channels[name] = {"avatar": write_image(photo, AVATARS_DIR, uuid.uuid4().hex[:8])}
            self.save()
            self.save_channels()

    def set_cover(self, item, data_url, auto):
        name = write_image(data_url, COVERS_DIR, item["id"])
        with self.lock:
            self._drop_cover_file(item)
            item["cover"], item["coverAuto"], item["autoTried"] = name, auto, True
            self.save()

    def remove_covers(self, items):
        with self.lock:
            for item in items:
                self._drop_cover_file(item)
                item["cover"], item["coverAuto"], item["autoTried"] = None, False, False
            self.save()

    def delete(self, items):
        with self.lock:
            for item in items:
                self._drop_cover_file(item)
                self.items.remove(item)
            self.save()
            gone = {i["id"] for i in items}
            for col in self.collections:
                col["videos"] = [v for v in col["videos"] if v not in gone]
            self.save_collections()

    def relink(self, pairs):
        """Points videos at a new file: pairs of (item, new_path). Skips files
        that are already in the library. Returns how many were relinked."""
        done = 0
        with self.lock:
            for item, new_path in pairs:
                new_path = os.path.abspath(new_path)
                key = os.path.normcase(new_path)
                if not os.path.isfile(new_path) or any(
                        i is not item and os.path.normcase(os.path.abspath(i["path"])) == key for i in self.items):
                    continue
                item["path"] = new_path
                if not item.get("cover"):
                    item["autoTried"] = False  # grab a frame now that the file is back
                done += 1
            if done:
                self.save()
        return done

    def missing_in(self, folder):
        """Missing videos whose file name exists in folder (e.g. a moved folder)."""
        with self.lock:
            return [i["id"] for i in self.items
                    if not os.path.isfile(i["path"]) and os.path.isfile(os.path.join(folder, Path(i["path"]).name))]

    @staticmethod
    def _drop_cover_file(item):
        if item.get("cover"):
            unlink_quietly(COVERS_DIR / item["cover"])


# --------------------------------------------------------------------------- native dialogs

# Windows' own Open dialog (IFileOpenDialog) through ctypes: no Tk to ship, and it opens
# on top of the app window as its owner.

dialog_lock = threading.Lock()


def _com_method(obj, index, *argtypes):
    """Method `index` of a COM object's vtable, callable as f(obj, *args) -> HRESULT."""
    import ctypes
    vtable = ctypes.cast(obj, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p)))[0]
    return ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, *argtypes)(vtable[index])


def native_file_dialog(mode, start_dir="", name="", owner=None):
    """mode: "files" (several), "locate" (one file) or "folder". Returns chosen paths ([] if cancelled)."""
    import ctypes
    from ctypes import wintypes
    ole32 = ctypes.windll.ole32
    ole32.CoTaskMemFree.argtypes = [ctypes.c_void_p]

    def guid(text):
        g = (ctypes.c_byte * 16)()
        ole32.CLSIDFromString(text, ctypes.byref(g))
        return g

    class FilterSpec(ctypes.Structure):
        _fields_ = [("name", wintypes.LPCWSTR), ("spec", wintypes.LPCWSTR)]

    def path_of(item):  # IShellItem::GetDisplayName(SIGDN_FILESYSPATH)
        out = ctypes.c_void_p()
        if _com_method(item, 5, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p))(item, 0x80058000, ctypes.byref(out)):
            return None
        try:
            return ctypes.wstring_at(out.value)
        finally:
            ole32.CoTaskMemFree(out)

    release = lambda obj: _com_method(obj, 2)(obj)
    ole32.CoInitializeEx(None, 0x2)  # apartment-threaded, as the dialog requires
    dialog = ctypes.c_void_p()
    try:
        if ole32.CoCreateInstance(ctypes.byref(guid("{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}")), None, 1,
                                  ctypes.byref(guid("{d57c7288-d4ad-4768-be02-9d969532d960}")), ctypes.byref(dialog)):
            return []
        options = ctypes.c_uint()
        _com_method(dialog, 10, ctypes.POINTER(ctypes.c_uint))(dialog, ctypes.byref(options))  # GetOptions
        options = options.value | 0x40 | 0x8  # FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR
        options |= {"folder": 0x20, "files": 0x200 | 0x1000}.get(mode, 0x1000)  # PICKFOLDERS / MULTISELECT, FILEMUSTEXIST
        _com_method(dialog, 9, ctypes.c_uint)(dialog, options)  # SetOptions
        if mode != "folder":
            specs = (FilterSpec * 2)(FilterSpec("Video files", ";".join(f"*{e}" for e in sorted(VIDEO_EXTS))),
                                     FilterSpec("All files", "*.*"))
            _com_method(dialog, 4, ctypes.c_uint, ctypes.POINTER(FilterSpec))(dialog, 2, specs)  # SetFileTypes
        title = {"files": "Add videos", "folder": "Add every video in a folder"}.get(mode, f'Locate "{name}"')
        _com_method(dialog, 17, wintypes.LPCWSTR)(dialog, title)  # SetTitle
        if start_dir:
            folder = ctypes.c_void_p()
            shell32 = ctypes.windll.shell32
            if not shell32.SHCreateItemFromParsingName(start_dir, None, ctypes.byref(
                    guid("{43826d1e-e718-42ee-bc55-a1e261c37bfe}")), ctypes.byref(folder)):
                _com_method(dialog, 12, ctypes.c_void_p)(dialog, folder)  # SetFolder
                release(folder)
        if name:
            _com_method(dialog, 15, wintypes.LPCWSTR)(dialog, name)  # SetFileName
        if _com_method(dialog, 3, wintypes.HWND)(dialog, owner):  # Show: non-zero = cancelled
            return []
        paths = []
        if mode == "files":  # GetResults -> IShellItemArray
            items, count = ctypes.c_void_p(), ctypes.c_uint()
            if not _com_method(dialog, 27, ctypes.POINTER(ctypes.c_void_p))(dialog, ctypes.byref(items)):
                _com_method(items, 7, ctypes.POINTER(ctypes.c_uint))(items, ctypes.byref(count))
                for n in range(count.value):
                    item = ctypes.c_void_p()
                    if not _com_method(items, 8, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p))(items, n, ctypes.byref(item)):
                        paths.append(path_of(item))
                        release(item)
                release(items)
        else:  # GetResult -> IShellItem
            item = ctypes.c_void_p()
            if not _com_method(dialog, 20, ctypes.POINTER(ctypes.c_void_p))(dialog, ctypes.byref(item)):
                paths.append(path_of(item))
                release(item)
        return [p for p in paths if p]
    finally:
        if dialog:
            release(dialog)
        ole32.CoUninitialize()


def ask_paths(mode, start_dir="", name=""):
    """mode: "files", "folder" (every video inside it) or "locate" (one file)."""
    owner = next(iter(app_windows()), None)  # the app window, so the dialog opens in front of it
    with dialog_lock:
        paths = native_file_dialog(mode, start_dir, name, owner)
    if mode == "folder" and paths:
        found = []
        for root, _dirs, files in os.walk(paths[0]):
            found += [os.path.join(root, f) for f in sorted(files)
                      if os.path.splitext(f)[1].lower() in VIDEO_EXTS]
        return found
    return paths


# --------------------------------------------------------------------------- http

class State:
    last_ping = time.time()
    close_at = None
    code_mtime = os.path.getmtime(__file__)  # to notice when app.py was updated
    restart = False


def server_outdated():
    try:
        return os.path.getmtime(__file__) > State.code_mtime
    except OSError:
        return False


def code_compiles():
    """Never restart into a half-saved app.py."""
    try:
        compile(Path(__file__).read_text(encoding="utf-8"), __file__, "exec")
        return True
    except (OSError, SyntaxError, ValueError):
        return False


def app_version():
    """Changes whenever the server restarts or a page file changes; the window reloads on a change."""
    try:
        web = max(f.stat().st_mtime for f in WEB_DIR.iterdir() if f.is_file())
    except (OSError, ValueError):
        web = 0
    return f"{State.code_mtime}-{web}"


def state_payload():
    """Everything the window shows; sent back after every change."""
    with library.lock:
        return {"library": library.public(), "settings": dict(settings),
                "collections": [dict(c, videos=list(c["videos"])) for c in library.collections]}


settings = load_json(SETTINGS_FILE, {})
SETTING_RULES = {"theme": lambda v: isinstance(v, str) and re.fullmatch(r"[a-z-]{1,32}", v),
                 "thumbSize": lambda v: isinstance(v, (int, float)) and 120 <= v <= 800}


def play_all(paths):
    """Opens the videos as one playlist in the default player (.m3u)."""
    with open(PLAYLIST_FILE, "w", encoding="utf-8") as f:
        f.write("#EXTM3U\n")
        for p in paths:
            f.write(f"#EXTINF:-1,{Path(p).stem}\n{p}\n")
    os.startfile(PLAYLIST_FILE)


class Handler(BaseHTTPRequestHandler):
    server_version = "VideoLibrary"

    def log_message(self, *args):
        pass

    # Only answer our own window: blocks other websites and DNS rebinding.
    def trusted(self):
        host = self.headers.get("Host", "")
        origin = self.headers.get("Origin")
        ok_hosts = {f"{HOST}:{PORT}", f"localhost:{PORT}"}
        return host in ok_hosts and (origin is None or origin in {f"http://{h}" for h in ok_hosts})

    def send_json(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_file(self, path, cache="no-cache"):
        if not path.is_file():
            return self.send_error(404)
        self.send_bytes(path.read_bytes(), FILE_TYPES.get(path.suffix.lower(), "application/octet-stream"), cache)

    def send_bytes(self, data, ctype, cache="no-cache"):
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", cache)
        self.end_headers()
        self.wfile.write(data)

    def send_media(self, path):
        size = os.path.getsize(path)
        start, end, status = 0, size - 1, 200
        m = re.match(r"bytes=(\d*)-(\d*)", self.headers.get("Range", ""))
        if m and (m.group(1) or m.group(2)):
            if m.group(1):
                start = int(m.group(1))
                end = min(int(m.group(2)), size - 1) if m.group(2) else size - 1
            else:
                start = max(0, size - int(m.group(2)))
            if start > end:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                return self.end_headers()
            status = 206
        ctype = MEDIA_TYPES.get(Path(path).suffix.lower(), "video/mp4")
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        try:
            with open(path, "rb") as f:
                f.seek(start)
                remaining = end - start + 1
                while remaining > 0:
                    chunk = f.read(min(256 * 1024, remaining))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    remaining -= len(chunk)
        except (ConnectionError, OSError):
            pass  # the video element routinely drops connections while seeking

    def do_GET(self):
        if not self.trusted():
            return self.send_error(403)
        path = unquote(urlparse(self.path).path)
        if path == "/api/library":
            return self.send_json(library.public())
        if path == "/api/state":
            return self.send_json(state_payload())
        if path == "/api/ping":
            return self.send_json({"app": "video-library"})
        if path.startswith("/covers/"):
            return self.send_file(COVERS_DIR / Path(path).name, cache="max-age=31536000, immutable")
        if path.startswith("/avatars/"):
            return self.send_file(AVATARS_DIR / Path(path).name, cache="max-age=31536000, immutable")
        if path.startswith("/media/"):
            item = library.get(Path(path).name)
            if not item or not os.path.isfile(item["path"]):
                return self.send_error(404)
            return self.send_media(item["path"])
        target = Path(os.path.abspath(WEB_DIR / (path.lstrip("/") or "index.html")))
        if WEB_DIR not in target.parents:
            return self.send_error(404)
        if target.name == "index.html":
            # Paint with the saved theme from the very first frame.
            page = target.read_text(encoding="utf-8").replace(
                "<html lang=\"en\">", f"<html lang=\"en\" data-theme=\"{settings.get('theme', 'classic-dark')}\">", 1)
            return self.send_bytes(page.encode("utf-8"), "text/html; charset=utf-8")
        return self.send_file(target)

    def collection_action(self, action, body):
        name = str(body.get("name", "")).strip()
        ids = [str(i) for i in body.get("ids", [])]
        if action == "create":
            if not name:
                return self.send_json({"error": "Give the collection a name"}, 400)
            col = library.create_collection(name, ids)
            return self.send_json({"id": col["id"], **state_payload()})

        col = library.collection(body.get("cid", ""))
        if not col:
            return self.send_json({"error": "Collection not found"}, 404)
        if action == "rename":
            if not name:
                return self.send_json({"error": "Give the collection a name"}, 400)
            with library.lock:
                col["name"], col["updated"] = name, time.time()
                library.save_collections()
        elif action == "delete":
            with library.lock:
                if col.get("cover"):
                    unlink_quietly(COVERS_DIR / col["cover"])
                library.collections.remove(col)
                library.save_collections()
        elif action in ("cover", "remove-cover"):
            # A custom cover; without one the first video's thumbnail is used.
            new = None
            if action == "cover":
                try:
                    new = write_image(body.get("image"), COVERS_DIR, f"col-{col['id']}")
                except ValueError as e:
                    return self.send_json({"error": str(e)}, 400)
            with library.lock:
                if col.get("cover"):
                    unlink_quietly(COVERS_DIR / col["cover"])
                col["cover"] = new
                library.save_collections()
        elif action == "add":
            library.collect(col, ids)
        elif action == "remove":
            library.uncollect(col, ids)
        elif action == "play":
            paths = [i["path"] for i in map(library.get, col["videos"]) if i and os.path.isfile(i["path"])]
            if not paths:
                return self.send_json({"error": "None of these videos can be found on your drives"}, 404)
            try:
                play_all(paths)
            except OSError:
                return self.send_json({"error": "No app is set to open playlists (.m3u). VLC can do it."}, 400)
            with library.lock:
                col["lastPlayed"] = time.time()
                library.save_collections()
            return self.send_json({"ok": True, "count": len(paths), **state_payload()})
        else:
            return self.send_json({"error": "Unknown action"}, 404)
        return self.send_json({"ok": True, **state_payload()})

    def do_POST(self):
        if not self.trusted():
            return self.send_error(403)
        path = urlparse(self.path).path
        length = int(self.headers.get("Content-Length") or 0)
        try:
            body = json.loads(self.rfile.read(length) or b"{}")
        except ValueError:
            return self.send_json({"error": "Bad request"}, 400)

        if path == "/api/ping":
            State.last_ping, State.close_at = time.time(), None
            return self.send_json({"ok": True, "outdated": server_outdated(), "version": app_version()})
        if path == "/api/bye":
            State.close_at = time.time() + CLOSE_GRACE
            return self.send_json({"ok": True})
        if path == "/api/settings":
            with library.lock:
                settings.update({k: v for k, v in body.items() if k in SETTING_RULES and SETTING_RULES[k](v)})
                write_json(SETTINGS_FILE, settings)
            return self.send_json({"ok": True})
        if path == "/api/add":
            picked = ask_paths("folder" if body.get("mode") == "folder" else "files")
            added = library.add_paths(picked)
            # Adding from inside a collection also puts the videos (new or not) in it.
            col = library.collection(body.get("collection", ""))
            if col and picked:
                library.collect(col, library.ids_for(picked))
            return self.send_json({"added": added, "picked": len(picked), **state_payload()})

        if path.startswith("/api/collection/"):
            return self.collection_action(path.rsplit("/", 1)[1], body)

        # Actions take one video ("id"); remove-cover, channel and delete also take several ("ids").
        items = [i for i in map(library.get, body.get("ids") or [body.get("id", "")]) if i]
        if not items:
            return self.send_json({"error": "Entry not found"}, 404)
        item = items[0]

        if path == "/api/open":
            if not os.path.isfile(item["path"]):
                return self.send_json({"error": "The file was moved or deleted"}, 404)
            os.startfile(item["path"])
            with library.lock:
                item["lastPlayed"] = time.time()
                library.save()
        elif path == "/api/reveal":
            target = item["path"] if os.path.isfile(item["path"]) else str(Path(item["path"]).parent)
            subprocess.Popen(["explorer", "/select,", os.path.normpath(target)])
        elif path == "/api/cover":
            try:
                library.set_cover(item, body.get("image"), auto=False)
            except ValueError as e:
                return self.send_json({"error": str(e)}, 400)
        elif path == "/api/auto-thumb":
            with library.lock:
                if body.get("duration"):
                    item["duration"] = float(body["duration"])
                item["autoTried"] = True
                item["thumbHQ"] = True  # captured at full size; older versions used 640px
                library.save()
            if body.get("image") and (not item.get("cover") or item.get("coverAuto")):
                library.set_cover(item, body["image"], auto=True)
        elif path == "/api/remove-cover":
            library.remove_covers(items)
        elif path == "/api/rename":
            title = str(body.get("title", "")).strip()
            if not title:
                return self.send_json({"error": "Title can't be empty"}, 400)
            with library.lock:
                item["title"] = title
                library.save()
        elif path == "/api/channel":
            name = str(body.get("name", "")).strip()
            if not name:
                return self.send_json({"error": "Channel name can't be empty"}, 400)
            try:
                apply_to_all = bool(body.get("applyToAll")) and len(items) == 1
                library.set_channel(items, name, body.get("photo"), apply_to_all)
            except ValueError as e:
                return self.send_json({"error": str(e)}, 400)
        elif path == "/api/watched":
            # The "watched" bar under the thumbnail.
            with library.lock:
                for i in items:
                    i["watched"] = bool(body.get("watched", True))
                library.save()
        elif path == "/api/delete":
            library.delete(items)
        elif path == "/api/locate":
            # Start the dialog in the closest folder that still exists.
            start = Path(item["path"]).parent
            while not start.exists() and start.parent != start:
                start = start.parent
            picked = ask_paths("locate", str(start) if start.exists() else "", Path(item["path"]).name)
            if not picked:
                return self.send_json({"relinked": 0, **state_payload()})
            if not library.relink([(item, picked[0])]):
                return self.send_json({"error": "That file is already in your library"}, 409)
            folder = os.path.dirname(os.path.abspath(picked[0]))
            return self.send_json({"relinked": 1, "folder": folder, "siblings": library.missing_in(folder),
                                   **state_payload()})
        elif path == "/api/relink-folder":
            # Relink missing videos to same-named files in a folder.
            folder = str(body.get("folder", ""))
            count = library.relink([(i, os.path.join(folder, Path(i["path"]).name))
                                    for i in items if not os.path.isfile(i["path"])])
            return self.send_json({"relinked": count, **state_payload()})
        else:
            return self.send_json({"error": "Unknown action"}, 404)
        return self.send_json({"ok": True, **state_payload()})


# --------------------------------------------------------------------------- startup

def open_window(url):
    candidates = [
        os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%ProgramFiles%\Google\Chrome\Application\chrome.exe"),
        os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
    ]
    for exe in candidates:
        if os.path.isfile(exe):
            # The app gets its own browser profile: the browser ignores a requested size and
            # position when it remembers one, and only in our own profile can we make it forget.
            forget_window_placement()
            args = [exe, f"--app={url}", f"--user-data-dir={BROWSER_PROFILE}", *BROWSER_FLAGS]
            bounds = startup_bounds()
            if bounds:
                x, y, w, h = bounds
                args += [f"--window-position={x},{y}", f"--window-size={w},{h}"]
            before = set(app_windows())
            subprocess.Popen(args)
            # Safety net: fixes the window only if it still opened somewhere else.
            threading.Thread(target=place_new_window, args=(before,), daemon=True).start()
            return
    import webbrowser  # no Edge or Chrome: use whatever browser is the default
    webbrowser.open(url)


# --------------------------------------------------------------------------- window placement (Windows)

# Keep the app's browser lean: none of the extensions, sync, downloads or disk caches a full browser uses.
BROWSER_FLAGS = [
    "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--disable-component-extensions-with-background-pages",
    "--disable-component-update", "--disable-background-networking", "--disable-sync", "--disable-default-apps",
    "--no-pings", "--disable-breakpad", "--disable-domain-reliability", "--metrics-recording-only",
    "--disk-cache-size=1", "--media-cache-size=1", "--disable-gpu-shader-disk-cache", "--disable-spell-checking",
    "--disable-features=msEdgeShopping,msEdgeSidebarV2,msEntityExtraction,EdgeCollections,msUndersideButton,"
    "msEdgeTranslate,Translate,OptimizationHints,MediaRouter,AutofillServerCommunication,SpareRendererForSitePerProcess",
]
WINDOW_TITLE = "Thumbnail"                # the page <title>
WINDOW_SIZE = (0.5, 0.6)                 # share of the screen's free area (width, height)
WINDOW_MIN = (1000, 680)                 # at 100% display scaling; grows with the scaling


def forget_window_placement():
    """Drops the window size/position the browser saved in the app's profile last time."""
    prefs = BROWSER_PROFILE / "Default" / "Preferences"
    try:
        data = json.loads(prefs.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    browser = data.get("browser", {})
    removed = [browser.pop(key, None) for key in ("app_window_placement", "window_placement")]
    if any(r is not None for r in removed):
        try:
            prefs.write_text(json.dumps(data), encoding="utf-8")
        except OSError:
            pass


def target_rect(work, scale):
    """(x, y, w, h) of a window centred in the work area (left, top, right, bottom)."""
    left, top, right, bottom = work
    free_w, free_h = right - left, bottom - top
    w = min(free_w, max(int(free_w * WINDOW_SIZE[0]), int(WINDOW_MIN[0] * scale)))
    h = min(free_h, max(int(free_h * WINDOW_SIZE[1]), int(WINDOW_MIN[1] * scale)))
    return left + (free_w - w) // 2, top + (free_h - h) // 2, w, h


def startup_bounds():
    """Where the window should open on the main screen, in the browser's units (DIPs)."""
    if sys.platform != "win32":
        return None
    try:
        ctypes, wintypes, user32 = _user32()
        work = wintypes.RECT()
        user32.SystemParametersInfoW(0x30, 0, ctypes.byref(work), 0)  # SPI_GETWORKAREA
        scale = user32.GetDpiForSystem() / 96 or 1
    except (AttributeError, OSError):
        return None
    x, y, w, h = target_rect((work.left, work.top, work.right, work.bottom), scale)
    return tuple(round(v / scale) for v in (x, y, w, h))


def _user32():
    import ctypes
    from ctypes import wintypes
    user32 = ctypes.windll.user32
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)  # work in real screen pixels
    except (AttributeError, OSError):
        pass
    user32.MonitorFromWindow.restype = wintypes.HMONITOR
    user32.MonitorFromWindow.argtypes = [wintypes.HWND, wintypes.DWORD]
    user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int,
                                    ctypes.c_int, ctypes.c_int, ctypes.c_uint]
    user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    return ctypes, wintypes, user32


def app_windows(title=WINDOW_TITLE, class_prefix="Chrome_WidgetWin"):
    """Visible top-level windows showing the app."""
    if sys.platform != "win32":
        return []
    ctypes, wintypes, user32 = _user32()
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def visit(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            text, cls = ctypes.create_unicode_buffer(256), ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, text, 256)
            user32.GetClassNameW(hwnd, cls, 256)
            if text.value == title and cls.value.startswith(class_prefix):
                found.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    return found


def center_window(hwnd):
    """Restores the window (if maximized) and centres it on its screen at WINDOW_SIZE."""
    ctypes, wintypes, user32 = _user32()

    class MONITORINFO(ctypes.Structure):
        _fields_ = [("cbSize", wintypes.DWORD), ("rcMonitor", wintypes.RECT),
                    ("rcWork", wintypes.RECT), ("dwFlags", wintypes.DWORD)]

    info = MONITORINFO(cbSize=ctypes.sizeof(MONITORINFO))
    user32.GetMonitorInfoW(user32.MonitorFromWindow(hwnd, 2), ctypes.byref(info))  # 2: nearest screen
    work = info.rcWork
    try:
        scale = user32.GetDpiForWindow(hwnd) / 96 or 1
    except (AttributeError, OSError):
        scale = 1
    x, y, w, h = target_rect((work.left, work.top, work.right, work.bottom), scale)
    # Already (about) there? Then leave it alone — no visible jump.
    now = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(now))
    close = all(abs(a - b) <= 16 for a, b in
                zip((now.left, now.top, now.right - now.left, now.bottom - now.top), (x, y, w, h)))
    if close and not user32.IsZoomed(hwnd):
        return False
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE: leave maximized / full screen
    user32.SetWindowPos(hwnd, None, x, y, w, h, 0x0004 | 0x0010)  # SWP_NOZORDER | SWP_NOACTIVATE
    return True


def place_new_window(before, timeout=20, **find):
    """Waits for the window the browser just opened, then centres it."""
    if sys.platform != "win32":
        return
    deadline = time.time() + timeout
    while time.time() < deadline:
        new = [h for h in app_windows(**find) if h not in before]
        if new:
            time.sleep(0.4)  # let the browser finish restoring its saved size first
            for hwnd in new:
                center_window(hwnd)
            return
        time.sleep(0.2)


def already_running():
    conn = http.client.HTTPConnection(HOST, PORT, timeout=2)
    try:
        conn.request("GET", "/api/ping")
        return json.loads(conn.getresponse().read()).get("app") == "video-library"
    except (OSError, ValueError):
        return False
    finally:
        conn.close()


def watchdog(server):
    while True:
        time.sleep(2)
        now = time.time()
        if (State.close_at and now > State.close_at) or now - State.last_ping > IDLE_TIMEOUT:
            server.shutdown()
            return
        # Updated while running: restart into the new code (not while a file dialog is open).
        if server_outdated() and code_compiles() and not dialog_lock.locked():
            State.restart = True
            server.shutdown()
            return


def bind_server(retries):
    for attempt in range(retries + 1):
        try:
            return ThreadingHTTPServer((HOST, PORT), Handler)
        except OSError:
            if attempt == retries:
                raise
            time.sleep(0.25)


def main():
    # --restarted: started by an earlier copy after an update; its window is already open.
    restarted = "--restarted" in sys.argv
    show_window = "--no-window" not in sys.argv and not restarted
    COVERS_DIR.mkdir(parents=True, exist_ok=True)
    AVATARS_DIR.mkdir(parents=True, exist_ok=True)
    if not restarted:
        library.prune_files()
    try:
        server = bind_server(retries=20 if restarted else 0)
    except OSError:
        if already_running():
            if show_window:
                open_window(ORIGIN)
            return
        raise
    server.daemon_threads = True
    # Drive letters can change (external drive on another PC): look for missing videos elsewhere.
    threading.Thread(target=library.reconnect_drives, daemon=True).start()
    if show_window or restarted:
        threading.Thread(target=watchdog, args=(server,), daemon=True).start()
    if show_window:
        open_window(ORIGIN)
    elif not restarted:
        print(f"Thumbnail running at {ORIGIN}")
    server.serve_forever()
    server.server_close()
    if State.restart:
        subprocess.Popen([sys.executable, os.path.abspath(__file__), "--restarted"], cwd=str(APP_DIR))


library = Library(LIBRARY_FILE, CHANNELS_FILE, COLLECTIONS_FILE)

if __name__ == "__main__":
    main()
