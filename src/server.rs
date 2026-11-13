//! The local web server the window talks to (127.0.0.1:8765): the page, the pictures,
//! the videos (for grabbing a cover frame) and the JSON API.

use crate::dialogs::{ask_paths, Mode};
use crate::library::{dirs, now, str_field, truthy, video_ids, Library, Obj};
use crate::{paths, platform, web};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tiny_http::{Header, Method, Request, Response, StatusCode};

pub const HOST: &str = "127.0.0.1";
pub const PORT: u16 = 8765;

type Resp = Response<Box<dyn Read + Send>>;

static LIBRARY: OnceLock<Mutex<Library>> = OnceLock::new();
static VERSION: OnceLock<String> = OnceLock::new();

pub fn init(library: Library) {
    let _ = LIBRARY.set(Mutex::new(library));
    let _ = VERSION.set(format!("rs-{}", now()));
}

pub fn lib() -> MutexGuard<'static, Library> {
    LIBRARY.get().expect("server::init first").lock().unwrap_or_else(|e| e.into_inner())
}

/// Answers requests until the process ends (each request on its own thread, so a long
/// video stream or an open file dialog never holds up the rest).
pub fn serve(server: tiny_http::Server) {
    for req in server.incoming_requests() {
        std::thread::spawn(move || handle(req));
    }
}

fn handle(mut req: Request) {
    let resp = if !trusted(&req) {
        plain(403)
    } else {
        match req.method() {
            Method::Get => get(&req),
            Method::Post => post(&mut req),
            _ => plain(501),
        }
    };
    let _ = req.respond(resp); // the video element routinely drops connections while seeking
}

// --------------------------------------------------------------------------- responses

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn bytes(status: u16, ctype: &str, data: Vec<u8>, cache: &str) -> Resp {
    let len = data.len();
    let headers = vec![header("Content-Type", ctype), header("Cache-Control", cache)];
    Response::new(StatusCode(status), headers, Box::new(std::io::Cursor::new(data)), Some(len), None)
}

fn json_resp(status: u16, v: Value) -> Resp {
    bytes(status, "application/json", serde_json::to_vec(&v).unwrap_or_default(), "no-store")
}

fn error(status: u16, msg: &str) -> Resp {
    json_resp(status, json!({ "error": msg }))
}

fn plain(status: u16) -> Resp {
    bytes(status, "text/plain; charset=utf-8", format!("Error {status}").into_bytes(), "no-store")
}

/// {extra..., library, settings, collections}
fn with_state(lib: &Library, extra: Value) -> Resp {
    let mut o = if let Value::Object(o) = extra { o } else { Obj::new() };
    o.extend(lib.state());
    json_resp(200, Value::Object(o))
}

fn ok_state(lib: &Library) -> Resp {
    with_state(lib, json!({ "ok": true }))
}

fn file_type(name: &str) -> &'static str {
    match paths::ext_lower(name).as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

fn media_type(path: &str) -> &'static str {
    match paths::ext_lower(path).as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "ts" | "m2ts" => "video/mp2t",
        "flv" => "video/x-flv",
        "wmv" => "video/x-ms-wmv",
        "avi" => "video/x-msvideo",
        "mpg" | "mpeg" | "vob" => "video/mpeg",
        "ogv" => "video/ogg",
        "3gp" => "video/3gpp",
        _ => "video/mp4",
    }
}

// --------------------------------------------------------------------------- request helpers

fn req_header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// Only answer our own window: blocks other websites and DNS rebinding.
fn trusted(req: &Request) -> bool {
    let ok_hosts = [format!("{HOST}:{PORT}"), format!("localhost:{PORT}")];
    let host = req_header(req, "Host").unwrap_or("");
    let origin_ok = match req_header(req, "Origin") {
        None => true,
        Some(o) => ok_hosts.iter().any(|h| o == format!("http://{h}")),
    };
    ok_hosts.iter().any(|h| h == host) && origin_ok
}

fn url_path(req: &Request) -> &str {
    let u = req.url();
    u.split(['?', '#']).next().unwrap_or(u)
}

/// %xx decoding (urllib.parse.unquote).
fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The last part of a URL path (like Path(path).name, which also splits on backslashes).
fn last_part(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or("")
}

// --------------------------------------------------------------------------- GET

fn get(req: &Request) -> Resp {
    let path = unquote(url_path(req));
    match path.as_str() {
        "/api/library" => return json_resp(200, Value::Array(lib().public())),
        "/api/state" => return json_resp(200, Value::Object(lib().state())),
        "/api/ping" => return json_resp(200, json!({ "app": "video-library" })),
        _ => {}
    }
    for (prefix, folder) in [("/covers/", &dirs().covers), ("/avatars/", &dirs().avatars)] {
        if path.starts_with(prefix) {
            let name = last_part(&path);
            return match std::fs::read(folder.join(name)) {
                Ok(data) if !name.is_empty() => bytes(200, file_type(name), data, "max-age=31536000, immutable"),
                _ => plain(404),
            };
        }
    }
    if path.starts_with("/media/") {
        let file = {
            let lib = lib();
            lib.idx(last_part(&path)).map(|i| crate::library::path_of(&lib.items[i]).to_string())
        };
        return match file {
            Some(f) if paths::is_file(&f) => send_media(req, &f),
            _ => plain(404),
        };
    }
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    match web::file(rel) {
        Some(data) if rel == "index.html" => {
            // Paint with the saved theme from the very first frame.
            let theme = str_field(&lib().settings, "theme").to_string();
            let theme = if theme.is_empty() { "classic-dark".to_string() } else { theme };
            let page = String::from_utf8_lossy(data)
                .replacen("<html lang=\"en\">", &format!("<html lang=\"en\" data-theme=\"{theme}\">"), 1);
            bytes(200, "text/html; charset=utf-8", page.into_bytes(), "no-cache")
        }
        Some(data) => bytes(200, file_type(rel), data.to_vec(), "no-cache"),
        None => plain(404),
    }
}

/// Parses "bytes=a-b" like the regex bytes=(\d*)-(\d*): (a, b) as digit strings.
fn parse_range(h: &str) -> Option<(String, String)> {
    let rest = h.strip_prefix("bytes=")?;
    let a: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let rest = rest[a.len()..].strip_prefix('-')?;
    let b: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((a, b))
}

/// A video file, with Range support so the page can seek in it.
fn send_media(req: &Request, path: &str) -> Resp {
    let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    let (mut start, mut end, mut status) = (0i64, size - 1, 200u16);
    if let Some((a, b)) = req_header(req, "Range").and_then(parse_range) {
        if !a.is_empty() || !b.is_empty() {
            let num = |s: &str| s.parse::<i64>().unwrap_or(i64::MAX);
            if !a.is_empty() {
                start = num(&a);
                end = if b.is_empty() { size - 1 } else { num(&b).min(size - 1) };
            } else {
                start = (size - num(&b)).max(0);
            }
            if start > end {
                let headers = vec![header("Content-Range", &format!("bytes */{size}"))];
                return Response::new(StatusCode(416), headers, Box::new(std::io::empty()), Some(0), None);
            }
            status = 206;
        }
    }
    let len = (end - start + 1).max(0) as u64;
    let mut headers = vec![header("Content-Type", media_type(path)), header("Accept-Ranges", "bytes")];
    if status == 206 {
        headers.push(header("Content-Range", &format!("bytes {start}-{end}/{size}")));
    }
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return plain(404),
    };
    if f.seek(SeekFrom::Start(start.max(0) as u64)).is_err() {
        return plain(404);
    }
    Response::new(StatusCode(status), headers, Box::new(f.take(len)), Some(len as usize), None)
}

// --------------------------------------------------------------------------- POST

/// A JSON value as a list of strings (non-strings are dropped).
fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn text(body: &Obj, k: &str) -> String {
    match body.get(k) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string().trim().to_string(),
    }
}

fn post(req: &mut Request) -> Resp {
    let path = url_path(req).to_string();
    let mut raw = Vec::new();
    if req.as_reader().read_to_end(&mut raw).is_err() {
        return error(400, "Bad request");
    }
    let body: Obj = if raw.iter().all(u8::is_ascii_whitespace) {
        Obj::new()
    } else {
        match serde_json::from_slice::<Value>(&raw) {
            Ok(Value::Object(o)) => o,
            Ok(_) => Obj::new(),
            Err(_) => return error(400, "Bad request"),
        }
    };

    match path.as_str() {
        // The Python version restarts itself after an update; this one never needs to.
        "/api/ping" => return json_resp(200, json!({ "ok": true, "outdated": false, "version": VERSION.get() })),
        "/api/bye" => return json_resp(200, json!({ "ok": true })),
        "/api/settings" => {
            let mut lib = lib();
            for (k, v) in &body {
                let ok = match k.as_str() {
                    "theme" => v.as_str().is_some_and(|s| {
                        (1..=32).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                    }),
                    "thumbSize" => v.as_f64().is_some_and(|n| (120.0..=800.0).contains(&n)),
                    _ => false,
                };
                if ok {
                    lib.settings.insert(k.clone(), v.clone());
                }
            }
            lib.save_settings();
            return json_resp(200, json!({ "ok": true }));
        }
        "/api/add" => {
            let mode = if str_field(&body, "mode") == "folder" { Mode::Folder } else { Mode::Files };
            let picked = ask_paths(mode, "", ""); // not under the library lock: the dialog can stay open
            let mut lib = lib();
            let added = lib.add_paths(&picked);
            // Adding from inside a collection also puts the videos (new or not) in it.
            if let Some(ci) = lib.col_idx(str_field(&body, "collection")) {
                if !picked.is_empty() {
                    let ids = lib.ids_for(&picked);
                    lib.collect(ci, &ids);
                }
            }
            return with_state(&lib, json!({ "added": added, "picked": picked.len() }));
        }
        _ => {}
    }
    if let Some(action) = path.strip_prefix("/api/collection/") {
        return collection_action(action, &body);
    }
    item_action(&path, &body)
}

fn collection_action(action: &str, body: &Obj) -> Resp {
    let name = text(body, "name");
    let ids = strings(body.get("ids"));
    let mut lib = lib();
    if action == "create" {
        if name.is_empty() {
            return error(400, "Give the collection a name");
        }
        let id = lib.create_collection(&name, &ids);
        return with_state(&lib, json!({ "id": id }));
    }
    let Some(ci) = lib.col_idx(str_field(body, "cid")) else {
        return error(404, "Collection not found");
    };
    match action {
        "rename" => {
            if name.is_empty() {
                return error(400, "Give the collection a name");
            }
            let col = &mut lib.collections[ci];
            col.insert("name".into(), json!(name));
            col.insert("updated".into(), json!(now()));
            lib.save_collections();
        }
        "delete" => {
            let col = lib.collections.remove(ci);
            let cover = str_field(&col, "cover");
            if !cover.is_empty() {
                crate::library::unlink_quietly(&dirs().covers.join(cover));
            }
            lib.save_collections();
        }
        "cover" | "remove-cover" => {
            // A custom cover; without one the first video's thumbnail is used.
            let mut new = Value::Null;
            if action == "cover" {
                let stem = format!("col-{}", str_field(&lib.collections[ci], "id"));
                match crate::library::write_image(str_field(body, "image"), &dirs().covers, &stem) {
                    Ok(n) => new = json!(n),
                    Err(e) => return error(400, &e),
                }
            }
            let old = str_field(&lib.collections[ci], "cover").to_string();
            if !old.is_empty() {
                crate::library::unlink_quietly(&dirs().covers.join(old));
            }
            lib.collections[ci].insert("cover".into(), new);
            lib.save_collections();
        }
        "add" => lib.collect(ci, &ids),
        "remove" => lib.uncollect(ci, &ids),
        "play" => {
            let files: Vec<String> = video_ids(&lib.collections[ci])
                .iter()
                .filter_map(|id| lib.idx(id))
                .map(|i| crate::library::path_of(&lib.items[i]).to_string())
                .filter(|p| paths::is_file(p))
                .collect();
            if files.is_empty() {
                return error(404, "None of these videos can be found on your drives");
            }
            if !platform::play_all(&files) {
                return error(400, "No app is set to open playlists (.m3u). VLC can do it.");
            }
            lib.collections[ci].insert("lastPlayed".into(), json!(now()));
            lib.save_collections();
            return with_state(&lib, json!({ "ok": true, "count": files.len() }));
        }
        _ => return error(404, "Unknown action"),
    }
    ok_state(&lib)
}

/// Actions on one video ("id"); remove-cover, channel, watched, hidden and delete also take several ("ids").
fn item_action(path: &str, body: &Obj) -> Resp {
    let wanted = if truthy(body.get("ids")) { strings(body.get("ids")) } else { vec![str_field(body, "id").to_string()] };

    // The two that open a file dialog must not hold the library lock while it is open.
    if path == "/api/locate" {
        return locate(&wanted);
    }

    let mut lib = lib();
    let idxs: Vec<usize> = wanted.iter().filter_map(|id| lib.idx(id)).collect();
    let Some(&first) = idxs.first() else {
        return error(404, "Entry not found");
    };
    let file = crate::library::path_of(&lib.items[first]).to_string();

    match path {
        "/api/open" => {
            if !paths::is_file(&file) {
                return error(404, "The file was moved or deleted");
            }
            if !platform::open(&file) {
                return error(400, "Windows has no app set to play this kind of file");
            }
            lib.items[first].insert("lastPlayed".into(), json!(now()));
            lib.save();
        }
        "/api/reveal" => {
            let target = if paths::is_file(&file) { file.clone() } else { paths::parent(&file) };
            platform::reveal(&paths::abspath(&target));
        }
        "/api/cover" => {
            if let Err(e) = lib.set_cover(first, str_field(body, "image"), false) {
                return error(400, &e);
            }
        }
        "/api/auto-thumb" => {
            {
                let item = &mut lib.items[first];
                if truthy(body.get("duration")) {
                    if let Some(d) = body.get("duration").and_then(Value::as_f64) {
                        item.insert("duration".into(), json!(d));
                    }
                }
                item.insert("autoTried".into(), json!(true));
                item.insert("thumbHQ".into(), json!(true)); // captured at full size; older versions used 640px
            }
            lib.save();
            let item = &lib.items[first];
            let may_replace = !truthy(item.get("cover")) || truthy(item.get("coverAuto"));
            if truthy(body.get("image")) && may_replace {
                if let Err(e) = lib.set_cover(first, str_field(body, "image"), true) {
                    return error(500, &e);
                }
            }
        }
        "/api/remove-cover" => lib.remove_covers(&idxs),
        "/api/rename" => {
            let title = text(body, "title");
            if title.is_empty() {
                return error(400, "Title can't be empty");
            }
            lib.items[first].insert("title".into(), json!(title));
            lib.save();
        }
        "/api/channel" => {
            let name = text(body, "name");
            if name.is_empty() {
                return error(400, "Channel name can't be empty");
            }
            let apply_to_all = truthy(body.get("applyToAll")) && idxs.len() == 1;
            let photo = body.get("photo").and_then(Value::as_str);
            if let Err(e) = lib.set_channel(&idxs, &name, photo, apply_to_all) {
                return error(400, &e);
            }
        }
        "/api/watched" => {
            // The "watched" bar under the thumbnail.
            let watched = body.get("watched").map(|v| truthy(Some(v))).unwrap_or(true);
            for &i in &idxs {
                lib.items[i].insert("watched".into(), json!(watched));
            }
            lib.save();
        }
        "/api/hidden" => {
            // Hidden videos stay in the library and in their collections; only Home leaves them out.
            let hidden = body.get("hidden").map(|v| truthy(Some(v))).unwrap_or(true);
            for &i in &idxs {
                if hidden {
                    lib.items[i].insert("hidden".into(), json!(true));
                } else {
                    lib.items[i].remove("hidden");
                }
            }
            lib.save();
        }
        "/api/delete" => lib.delete(&idxs),
        "/api/relink-folder" => {
            // Relink missing videos to same-named files in a folder.
            let folder = text(body, "folder");
            let pairs: Vec<(usize, String)> = idxs
                .iter()
                .map(|&i| (i, crate::library::path_of(&lib.items[i]).to_string()))
                .filter(|(_, p)| !paths::is_file(p))
                .map(|(i, p)| (i, format!("{folder}\\{}", paths::file_name(&p))))
                .collect();
            let count = lib.relink(&pairs);
            return with_state(&lib, json!({ "relinked": count }));
        }
        _ => return error(404, "Unknown action"),
    }
    ok_state(&lib)
}

/// "Locate file": pick where a missing video went, then offer to fix its neighbours too.
fn locate(wanted: &[String]) -> Resp {
    let (id, file) = {
        let lib = lib();
        let Some(i) = wanted.iter().find_map(|id| lib.idx(id)) else {
            return error(404, "Entry not found");
        };
        (str_field(&lib.items[i], "id").to_string(), crate::library::path_of(&lib.items[i]).to_string())
    };
    // Start the dialog in the closest folder that still exists.
    let mut start = std::path::PathBuf::from(paths::parent(&file));
    while !start.exists() {
        match start.parent() {
            Some(p) if p != start => start = p.to_path_buf(),
            _ => break,
        }
    }
    let start = if start.exists() { start.to_string_lossy().into_owned() } else { String::new() };
    let picked = ask_paths(Mode::Locate, &start, &paths::file_name(&file));

    let mut lib = lib();
    let Some(i) = lib.idx(&id) else {
        return error(404, "Entry not found");
    };
    let Some(choice) = picked.first() else {
        return with_state(&lib, json!({ "relinked": 0 }));
    };
    if lib.relink(&[(i, choice.clone())]) == 0 {
        return error(409, "That file is already in your library");
    }
    let folder = paths::parent(&paths::abspath(choice));
    let siblings = lib.missing_in(&folder);
    with_state(&lib, json!({ "relinked": 1, "folder": folder, "siblings": siblings }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-"), Some(("0".into(), "".into())));
        assert_eq!(parse_range("bytes=10-20"), Some(("10".into(), "20".into())));
        assert_eq!(parse_range("bytes=-500"), Some(("".into(), "500".into())));
        assert_eq!(parse_range("bytes=x"), None);
    }

    #[test]
    fn unquoting() {
        assert_eq!(unquote("/media/abc"), "/media/abc");
        assert_eq!(unquote("/covers/a%20b.jpg"), "/covers/a b.jpg");
        assert_eq!(unquote("/x%C3%A9"), "/xé");
        assert_eq!(unquote("/bad%2"), "/bad%2");
    }
}
