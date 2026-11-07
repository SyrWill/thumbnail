//! The video library: videos, channels, collections and settings, saved as JSON in data\.
//! Same files and fields as the Python version, so either one can open the other's data.

use crate::paths;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Obj = Map<String, Value>;

pub const VIDEO_EXTS: &[&str] = &[
    "3gp", "avi", "flv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "ogv", "ts", "vob", "webm", "wmv",
];

pub fn image_ext(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "image/bmp" => ".bmp",
        "image/avif" => ".avif",
        _ => return None,
    })
}

// --------------------------------------------------------------------------- folders

pub struct Dirs {
    pub root: String, // the portable folder (where Thumbnail.exe is)
    pub data: PathBuf,
    pub covers: PathBuf,
    pub avatars: PathBuf,
    pub library: PathBuf,
    pub channels: PathBuf,
    pub collections: PathBuf,
    pub settings: PathBuf,
    pub playlist: PathBuf,
}

static DIRS: OnceLock<Dirs> = OnceLock::new();

pub fn init_dirs(root: &Path) {
    let data = root.join("data");
    let _ = DIRS.set(Dirs {
        root: root.to_string_lossy().into_owned(),
        covers: data.join("covers"),
        avatars: data.join("avatars"),
        library: data.join("library.json"),
        channels: data.join("channels.json"),
        collections: data.join("collections.json"),
        settings: data.join("settings.json"),
        playlist: data.join("play-all.m3u"),
        data,
    });
}

pub fn dirs() -> &'static Dirs {
    DIRS.get().expect("init_dirs first")
}

// --------------------------------------------------------------------------- small helpers

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// n random hex characters (ids; like uuid4().hex[:n]).
pub fn rand_hex(n: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < n {
        // RandomState is seeded randomly per instance by the OS.
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out.truncate(n);
    out
}

/// Python truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

pub fn str_field<'a>(o: &'a Obj, k: &str) -> &'a str {
    o.get(k).and_then(Value::as_str).unwrap_or("")
}

pub fn unlink_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Reads a JSON file. A broken file is kept aside (name.broken-<time>.json) and the default used.
/// The bool says whether the file could be used (a missing file counts as fine).
pub fn load_json(path: &Path, default: Value) -> (Value, bool) {
    if !path.exists() {
        return (default, true);
    }
    match fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(v) if std::mem::discriminant(&v) == std::mem::discriminant(&default) => (v, true),
        _ => {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let broken = path.with_file_name(format!("{stem}.broken-{}.json", now() as u64));
            let _ = fs::copy(path, broken);
            (default, false)
        }
    }
}

/// Writes JSON through a temp file, so a crash never leaves half a file behind.
pub fn write_json(path: &Path, data: &Value) {
    let tmp = path.with_extension("tmp");
    if let Ok(text) = serde_json::to_string_pretty(data) {
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

/// Saves a base64 data URL into folder and returns the new file name.
pub fn write_image(data_url: &str, folder: &Path, stem: &str) -> Result<String, String> {
    let bad = || "Unsupported image".to_string();
    let rest = data_url.strip_prefix("data:").ok_or_else(bad)?;
    let (mime, b64) = rest.split_once(";base64,").ok_or_else(bad)?;
    let mime_ok = !mime.is_empty() && mime.chars().all(|c| c.is_alphanumeric() || "_/+.-".contains(c));
    let ext = image_ext(mime).filter(|_| mime_ok).ok_or_else(bad)?;
    let clean: String = b64.chars().filter(|c| c.is_ascii_alphanumeric() || "+/".contains(*c)).collect();
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let bytes = engine.decode(clean).map_err(|_| bad())?;
    let name = format!("{stem}-{}{ext}", now_ms());
    fs::write(folder.join(&name), bytes).map_err(|e| e.to_string())?;
    Ok(name)
}

pub fn path_of(item: &Obj) -> &str {
    str_field(item, "path")
}

/// A video belongs to its folder's "channel" until you name one.
pub fn channel_of(item: &Obj) -> String {
    match item.get("channel").and_then(Value::as_str) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => paths::folder_of(path_of(item)),
    }
}

fn objects(v: Value) -> Vec<Obj> {
    match v {
        Value::Array(a) => a
            .into_iter()
            .filter_map(|x| if let Value::Object(o) = x { Some(o) } else { None })
            .collect(),
        _ => Vec::new(),
    }
}

fn object(v: Value) -> Obj {
    if let Value::Object(o) = v { o } else { Obj::new() }
}

/// The ids in a collection's "videos" list.
pub fn video_ids(col: &Obj) -> Vec<String> {
    col.get("videos")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn without(col: &mut Obj, gone: &HashSet<String>) {
    let kept: Vec<Value> = video_ids(col).into_iter().filter(|v| !gone.contains(v)).map(Value::String).collect();
    col.insert("videos".into(), Value::Array(kept));
}

// --------------------------------------------------------------------------- library

pub struct Library {
    pub items: Vec<Obj>,
    pub channels: Obj,         // name -> {"avatar": file}
    pub collections: Vec<Obj>, // [{"id", "name", "videos": [ids in order], "created", "updated", ...}]
    pub settings: Obj,
    /// Every file read fine: only then is it safe to delete pictures nothing points to.
    loaded_ok: bool,
}

impl Library {
    pub fn load() -> Self {
        let d = dirs();
        let (items, ok1) = load_json(&d.library, json!([]));
        let (channels, ok2) = load_json(&d.channels, json!({}));
        let (collections, ok3) = load_json(&d.collections, json!([]));
        let (settings, _) = load_json(&d.settings, json!({}));
        let mut items = objects(items);
        for i in &mut items {
            // In memory, paths are always absolute.
            let abs = paths::from_disk_path(path_of(i), &d.root);
            i.insert("path".into(), Value::String(abs));
        }
        Library {
            items,
            channels: object(channels),
            collections: objects(collections),
            settings: object(settings),
            loaded_ok: ok1 && ok2 && ok3,
        }
    }

    /// Deletes covers and photos nothing points to any more, and leftover temp files.
    pub fn prune_files(&self) {
        if !self.loaded_ok {
            return; // a broken library file: keep every picture until it is sorted out
        }
        let d = dirs();
        let mut used: HashSet<String> = HashSet::new();
        for i in self.items.iter().chain(self.collections.iter()) {
            used.insert(str_field(i, "cover").to_string());
        }
        for ch in self.channels.values() {
            if let Some(a) = ch.get("avatar").and_then(Value::as_str) {
                used.insert(a.to_string());
            }
        }
        for folder in [&d.covers, &d.avatars] {
            for e in fs::read_dir(folder).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.file_type().is_ok_and(|t| t.is_file()) && !used.contains(&name) {
                    unlink_quietly(&e.path());
                }
            }
        }
        for e in fs::read_dir(&d.data).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().ends_with(".tmp") {
                unlink_quietly(&e.path());
            }
        }
    }

    /// Missing videos that turn up under another drive letter are pointed there. Returns how many.
    pub fn reconnect_drives(&mut self) -> usize {
        let mut moved = 0;
        for i in &mut self.items {
            let p = path_of(i).to_string();
            if !paths::is_file(&p) {
                if let Some(found) = paths::find_on_other_drive(&p) {
                    i.insert("path".into(), Value::String(found));
                    moved += 1;
                }
            }
        }
        if moved > 0 {
            self.save();
        }
        moved
    }

    pub fn save(&self) {
        let root = &dirs().root;
        let out: Vec<Value> = self
            .items
            .iter()
            .map(|i| {
                let mut o = i.clone();
                o.insert("path".into(), Value::String(paths::to_disk_path(path_of(i), root)));
                Value::Object(o)
            })
            .collect();
        write_json(&dirs().library, &Value::Array(out));
    }

    pub fn save_channels(&self) {
        write_json(&dirs().channels, &Value::Object(self.channels.clone()));
    }

    pub fn save_collections(&self) {
        let cols = self.collections.iter().cloned().map(Value::Object).collect();
        write_json(&dirs().collections, &Value::Array(cols));
    }

    pub fn save_settings(&self) {
        write_json(&dirs().settings, &Value::Object(self.settings.clone()));
    }

    pub fn idx(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|i| str_field(i, "id") == id)
    }

    pub fn col_idx(&self, cid: &str) -> Option<usize> {
        self.collections.iter().position(|c| str_field(c, "id") == cid)
    }

    // ---- collections

    pub fn create_collection(&mut self, name: &str, ids: &[String]) -> String {
        let t = now();
        let id = rand_hex(10);
        self.collections.push(object(json!({"id": id, "name": name, "videos": [], "created": t, "updated": t})));
        let ci = self.collections.len() - 1;
        self.collect(ci, ids); // also saves
        id
    }

    /// Appends videos to a collection, skipping ones already in it.
    pub fn collect(&mut self, ci: usize, ids: &[String]) {
        let known: HashSet<&str> = self.items.iter().map(|i| str_field(i, "id")).collect();
        let mut videos = video_ids(&self.collections[ci]);
        for v in ids {
            if known.contains(v.as_str()) && !videos.contains(v) {
                videos.push(v.clone());
            }
        }
        let col = &mut self.collections[ci];
        col.insert("videos".into(), json!(videos));
        col.insert("updated".into(), json!(now()));
        self.save_collections();
    }

    pub fn uncollect(&mut self, ci: usize, ids: &[String]) {
        let gone: HashSet<String> = ids.iter().cloned().collect();
        let col = &mut self.collections[ci];
        without(col, &gone);
        col.insert("updated".into(), json!(now()));
        self.save_collections();
    }

    // ---- videos

    /// Library ids of these files (new or already added), in the given order.
    pub fn ids_for(&self, picked: &[String]) -> Vec<String> {
        let by_path: HashMap<String, &str> =
            self.items.iter().map(|i| (paths::key(path_of(i)), str_field(i, "id"))).collect();
        picked.iter().filter_map(|p| by_path.get(&paths::key(p)).map(|s| s.to_string())).collect()
    }

    pub fn add_paths(&mut self, picked: &[String]) -> Vec<String> {
        let mut known: HashSet<String> = self.items.iter().map(|i| paths::key(path_of(i))).collect();
        let mut added = Vec::new();
        for p in picked {
            let p = paths::abspath(p);
            let k = paths::normcase(&p);
            if known.contains(&k) || !paths::is_file(&p) {
                continue;
            }
            known.insert(k);
            let id = rand_hex(12);
            self.items.push(object(json!({
                "id": id, "path": p, "title": paths::stem(&p), "cover": null, "coverAuto": false,
                "autoTried": false, "duration": null, "added": now(), "lastPlayed": null,
            })));
            added.push(id);
        }
        if !added.is_empty() {
            self.save();
        }
        added
    }

    /// The videos as the window sees them (plus size, folder, channel, photo and type).
    pub fn public(&self) -> Vec<Value> {
        self.items
            .iter()
            .map(|i| {
                let p = path_of(i);
                let size = fs::metadata(p).ok().map(|m| m.len()); // one disk query per video
                let channel = channel_of(i);
                let avatar = self.channels.get(&channel).and_then(|c| c.get("avatar")).cloned().unwrap_or(Value::Null);
                let mut o = i.clone();
                o.insert("exists".into(), json!(size.is_some()));
                o.insert("size".into(), json!(size));
                o.insert("folder".into(), json!(paths::folder_of(p)));
                o.insert("channel".into(), json!(channel));
                o.insert("channelAvatar".into(), avatar);
                o.insert("ext".into(), json!(paths::ext_upper(p)));
                Value::Object(o)
            })
            .collect()
    }

    /// Everything the window shows; sent back after every change.
    pub fn state(&self) -> Obj {
        let mut o = Obj::new();
        o.insert("library".into(), Value::Array(self.public()));
        o.insert("settings".into(), Value::Object(self.settings.clone()));
        o.insert("collections".into(), Value::Array(self.collections.iter().cloned().map(Value::Object).collect()));
        o
    }

    /// Moves the videos (or every video of a single video's channel) to channel `name`.
    /// photo: None keeps the photo, "remove" clears it, or a data URL.
    pub fn set_channel(&mut self, idxs: &[usize], name: &str, photo: Option<&str>, apply_to_all: bool) -> Result<(), String> {
        let olds: HashSet<String> = idxs.iter().map(|&i| channel_of(&self.items[i])).collect();
        let targets: Vec<usize> = if apply_to_all {
            (0..self.items.len()).filter(|&i| olds.contains(&channel_of(&self.items[i]))).collect()
        } else {
            idxs.to_vec()
        };
        for i in targets {
            self.items[i].insert("channel".into(), json!(name));
        }
        // Renaming a whole channel takes its photo along.
        if apply_to_all && olds.len() == 1 {
            let old = olds.iter().next().cloned().unwrap_or_default();
            if !old.is_empty() && name != old && self.channels.contains_key(&old) && !self.channels.contains_key(name) {
                if let Some(v) = self.channels.shift_remove(&old) {
                    self.channels.insert(name.to_string(), v);
                }
            }
        }
        let mut result = Ok(());
        if let Some(photo) = photo.filter(|p| !p.is_empty()) {
            let current = self.channels.shift_remove(name);
            if let Some(a) = current.as_ref().and_then(|c| c.get("avatar")).and_then(Value::as_str) {
                if !a.is_empty() {
                    unlink_quietly(&dirs().avatars.join(a));
                }
            }
            if photo != "remove" {
                match write_image(photo, &dirs().avatars, &rand_hex(8)) {
                    Ok(file) => {
                        self.channels.insert(name.to_string(), json!({"avatar": file}));
                    }
                    Err(e) => result = Err(e),
                }
            }
        }
        self.save();
        self.save_channels();
        result
    }

    fn drop_cover_file(item: &Obj) {
        let c = str_field(item, "cover");
        if !c.is_empty() {
            unlink_quietly(&dirs().covers.join(c));
        }
    }

    pub fn set_cover(&mut self, i: usize, data_url: &str, auto: bool) -> Result<(), String> {
        let stem = str_field(&self.items[i], "id").to_string();
        let name = write_image(data_url, &dirs().covers, &stem)?;
        let item = &mut self.items[i];
        Self::drop_cover_file(item);
        item.insert("cover".into(), json!(name));
        item.insert("coverAuto".into(), json!(auto));
        item.insert("autoTried".into(), json!(true));
        self.save();
        Ok(())
    }

    pub fn remove_covers(&mut self, idxs: &[usize]) {
        for &i in idxs {
            let item = &mut self.items[i];
            Self::drop_cover_file(item);
            item.insert("cover".into(), Value::Null);
            item.insert("coverAuto".into(), json!(false));
            item.insert("autoTried".into(), json!(false));
        }
        self.save();
    }

    pub fn delete(&mut self, idxs: &[usize]) {
        let mut idxs = idxs.to_vec();
        idxs.sort_unstable();
        idxs.dedup();
        let mut gone = HashSet::new();
        for &i in idxs.iter().rev() {
            let item = self.items.remove(i);
            Self::drop_cover_file(&item);
            gone.insert(str_field(&item, "id").to_string());
        }
        self.save();
        for col in &mut self.collections {
            without(col, &gone);
        }
        self.save_collections();
    }

    /// Points videos at a new file: (index, new path). Skips files that are already in
    /// the library. Returns how many were relinked.
    pub fn relink(&mut self, pairs: &[(usize, String)]) -> usize {
        let mut done = 0;
        for (i, new_path) in pairs {
            let new_path = paths::abspath(new_path);
            let k = paths::normcase(&new_path);
            let taken = self.items.iter().enumerate().any(|(j, it)| j != *i && paths::key(path_of(it)) == k);
            if !paths::is_file(&new_path) || taken {
                continue;
            }
            let item = &mut self.items[*i];
            item.insert("path".into(), json!(new_path));
            if !truthy(item.get("cover")) {
                item.insert("autoTried".into(), json!(false)); // grab a frame now that the file is back
            }
            done += 1;
        }
        if done > 0 {
            self.save();
        }
        done
    }

    /// Missing videos whose file name exists in folder (e.g. a moved folder).
    pub fn missing_in(&self, folder: &str) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| {
                let p = path_of(i);
                !paths::is_file(p) && paths::is_file(&format!("{folder}\\{}", paths::file_name(p)))
            })
            .map(|i| str_field(i, "id").to_string())
            .collect()
    }
}
