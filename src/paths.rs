//! Windows path helpers that behave like Python's os.path (ntpath), so the library.json
//! written by the Python version and this one stay interchangeable.

use std::path::{Component, Path};

/// Absolute, normalized path (GetFullPathNameW, like os.path.abspath). No disk access.
pub fn abspath(p: &str) -> String {
    let p = if p.is_empty() { "." } else { p };
    std::path::absolute(p)
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string())
}

/// os.path.normcase on Windows: lower case with backslashes.
pub fn normcase(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

/// The key two paths are compared by (normcase(abspath(p))).
pub fn key(p: &str) -> String {
    normcase(&abspath(p))
}

fn is_sep(c: u8) -> bool {
    c == b'\\' || c == b'/'
}

/// os.path.splitdrive: ("D:", rest) or ("\\server\share", rest) or ("", p).
pub fn splitdrive(p: &str) -> (&str, &str) {
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return (&p[..2], &p[2..]);
    }
    if b.len() > 2 && is_sep(b[0]) && is_sep(b[1]) && !is_sep(b[2]) {
        if let Some(i) = p[2..].find(['\\', '/']).map(|i| i + 2) {
            let j = p[i + 1..].find(['\\', '/']).map(|j| j + i + 1).unwrap_or(p.len());
            if j > i + 1 {
                return (&p[..j], &p[j..]);
            }
        }
    }
    ("", p)
}

/// os.path.isabs as in Python 3.13+: "X:\..." or a UNC path.
pub fn isabs(p: &str) -> bool {
    let head: String = p.chars().take(3).collect::<String>().replace('/', "\\");
    head.get(1..3) == Some(":\\") || head.starts_with("\\\\")
}

/// os.path.relpath(path, start). None when they are on different drives.
pub fn relpath(path: &str, start: &str) -> Option<String> {
    let (path_abs, start_abs) = (abspath(path), abspath(start));
    let (pd, prest) = splitdrive(&path_abs);
    let (sd, srest) = splitdrive(&start_abs);
    if normcase(pd) != normcase(sd) {
        return None;
    }
    let split = |s: &str| s.split(['\\', '/']).filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let (pl, sl) = (split(prest), split(srest));
    let common = pl.iter().zip(&sl).take_while(|(a, b)| normcase(a) == normcase(b)).count();
    let mut parts: Vec<String> = vec!["..".to_string(); sl.len() - common];
    parts.extend(pl[common..].iter().cloned());
    Some(if parts.is_empty() { ".".to_string() } else { parts.join("\\") })
}

/// Videos on the app's own drive are saved relative to the app folder (..\Videos\x.mp4),
/// so the portable folder keeps working when the drive letter changes.
pub fn to_disk_path(path: &str, root: &str) -> String {
    if splitdrive(path).0.to_lowercase() == splitdrive(root).0.to_lowercase() {
        if let Some(rel) = relpath(path, root) {
            return rel;
        }
    }
    path.to_string()
}

pub fn from_disk_path(path: &str, root: &str) -> String {
    if isabs(path) { path.to_string() } else { abspath(&format!("{root}\\{path}")) }
}

pub fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn stem(p: &str) -> String {
    Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn parent(p: &str) -> String {
    Path::new(p).parent().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

/// Drive and root, e.g. "D:\" (Path.anchor).
fn anchor(p: &str) -> String {
    let mut out = String::new();
    for c in Path::new(p).components() {
        match c {
            Component::Prefix(pr) => out.push_str(&pr.as_os_str().to_string_lossy()),
            Component::RootDir => out.push('\\'),
            _ => break,
        }
    }
    out
}

/// Name of the folder a file is in, or the drive ("D:\") for files at the top of a drive.
pub fn folder_of(p: &str) -> String {
    match Path::new(p).parent().and_then(|q| q.file_name()) {
        Some(name) => name.to_string_lossy().into_owned(),
        None => anchor(p),
    }
}

/// "MP4" for "x.mp4".
pub fn ext_upper(p: &str) -> String {
    Path::new(p).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default()
}

pub fn ext_lower(p: &str) -> String {
    Path::new(p).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_file(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_file()
}

/// The same file under another drive letter (a drive that came back as G: instead of F:).
pub fn find_on_other_drive(path: &str) -> Option<String> {
    let (drive, rest) = splitdrive(path);
    if drive.len() != 2 || !drive.ends_with(':') {
        return None;
    }
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:", (b'A' + i) as char))
        .filter(|d| !d.eq_ignore_ascii_case(drive))
        .map(|d| format!("{d}{rest}"))
        .find(|c| is_file(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relpath_like_python() {
        assert_eq!(relpath(r"D:\Videos\a.mp4", r"D:\Apps\Thumbnail").as_deref(), Some(r"..\..\Videos\a.mp4"));
        assert_eq!(relpath(r"d:\apps\thumbnail\x.mp4", r"D:\Apps\Thumbnail").as_deref(), Some("x.mp4"));
        assert_eq!(relpath(r"E:\a.mp4", r"D:\Apps").as_deref(), None);
    }

    #[test]
    fn disk_paths_round_trip() {
        let root = r"D:\Apps\Thumbnail";
        let rel = to_disk_path(r"D:\Videos\a b.mp4", root);
        assert_eq!(rel, r"..\..\Videos\a b.mp4");
        assert_eq!(from_disk_path(&rel, root), r"D:\Videos\a b.mp4");
        assert_eq!(to_disk_path(r"E:\x.mp4", root), r"E:\x.mp4");
        assert_eq!(from_disk_path(r"E:\x.mp4", root), r"E:\x.mp4");
    }

    #[test]
    fn names() {
        assert_eq!(folder_of(r"D:\Videos\Kids\a.mp4"), "Kids");
        assert_eq!(folder_of(r"D:\a.mp4"), r"D:\");
        assert_eq!(ext_upper(r"D:\a.Mp4"), "MP4");
        assert_eq!(stem(r"D:\x\My Video.final.mkv"), "My Video.final");
        assert!(isabs(r"C:\x") && isabs(r"\\server\share\x") && !isabs(r"..\x") && !isabs(r"\x"));
        assert_eq!(splitdrive(r"\\srv\share\dir\f"), (r"\\srv\share", r"\dir\f"));
    }
}
