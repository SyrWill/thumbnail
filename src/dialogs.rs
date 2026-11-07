//! Windows' own Open dialog (through rfd), opened in front of the app window.

use crate::library::VIDEO_EXTS;
use crate::paths;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::num::NonZeroIsize;
use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

/// The app window, so dialogs open in front of it as its owner.
pub static OWNER: AtomicIsize = AtomicIsize::new(0);
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Files,  // several videos
    Folder, // every video in a folder (and its subfolders)
    Locate, // one file
}

struct Owner(NonZeroIsize);

impl HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Owner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

/// Returns the chosen files ([] if cancelled). For Folder: every video inside it.
pub fn ask_paths(mode: Mode, start_dir: &str, name: &str) -> Vec<String> {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let title = match mode {
        Mode::Files => "Add videos".to_string(),
        Mode::Folder => "Add every video in a folder".to_string(),
        Mode::Locate => format!("Locate \"{name}\""),
    };
    let mut d = rfd::FileDialog::new().set_title(title);
    if mode != Mode::Folder {
        d = d.add_filter("Video files", VIDEO_EXTS).add_filter("All files", &["*"]);
    }
    if !start_dir.is_empty() {
        d = d.set_directory(start_dir);
    }
    if !name.is_empty() {
        d = d.set_file_name(name);
    }
    if let Some(h) = NonZeroIsize::new(OWNER.load(Ordering::Relaxed)) {
        d = d.set_parent(&Owner(h));
    }
    let to_s = |p: std::path::PathBuf| p.to_string_lossy().into_owned();
    match mode {
        Mode::Files => d.pick_files().unwrap_or_default().into_iter().map(to_s).collect(),
        Mode::Locate => d.pick_file().map(to_s).into_iter().collect(),
        Mode::Folder => d.pick_folder().map(|f| videos_in(&f)).unwrap_or_default(),
    }
}

/// Every video under folder, like os.walk: a folder's own files (sorted) before its subfolders.
fn videos_in(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return found;
    };
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for e in entries.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push(e.path()),
            Ok(t) if t.is_file() => files.push(e.path()),
            _ => {}
        }
    }
    files.sort();
    dirs.sort();
    for f in files {
        let s = f.to_string_lossy().into_owned();
        if VIDEO_EXTS.contains(&paths::ext_lower(&s).as_str()) {
            found.push(s);
        }
    }
    for d in dirs {
        found.extend(videos_in(&d));
    }
    found
}
