//! Small Windows calls: open with the default app, show in Explorer, message boxes,
//! bring an open window to the front, and where a new window should go.

use crate::library::dirs;
use crate::paths;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, MessageBoxW, SetForegroundWindow, ShowWindow, SystemParametersInfoW, MB_ICONERROR,
    SPI_GETWORKAREA, SW_RESTORE, SW_SHOWNORMAL,
};

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Opens a file with its default app (os.startfile). False if Windows has no app for it.
pub fn open(path: &str) -> bool {
    let (verb, file) = (wide("open"), wide(path));
    let result = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
    };
    result as isize > 32
}

/// Shows the file selected in an Explorer window.
pub fn reveal(path: &str) {
    let _ = std::process::Command::new("explorer").arg("/select,").arg(path).spawn();
}

/// Opens the videos as one playlist in the default player (.m3u).
pub fn play_all(files: &[String]) -> bool {
    let mut text = String::from("#EXTM3U\r\n");
    for p in files {
        text.push_str(&format!("#EXTINF:-1,{}\r\n{}\r\n", paths::stem(p), p));
    }
    let list = &dirs().playlist;
    std::fs::write(list, text).is_ok() && open(&list.to_string_lossy())
}

pub fn error_box(text: &str) {
    let (t, c) = (wide(text), wide("Thumbnail"));
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), MB_ICONERROR) };
}

/// Brings the window called `title` to the front. False if there is none.
pub fn focus_window(title: &str) -> bool {
    let t = wide(title);
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), t.as_ptr());
        if hwnd.is_null() {
            return false;
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
    true
}

pub const WINDOW_SIZE: (f64, f64) = (0.5, 0.6); // share of the screen's free area (width, height)
pub const WINDOW_MIN: (f64, f64) = (1000.0, 680.0); // at 100% display scaling; grows with the scaling

/// (x, y, w, h) in screen pixels: centred in the main screen's free area (without the taskbar).
pub fn startup_bounds() -> Option<(i32, i32, i32, i32)> {
    let mut work = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0) };
    if ok == 0 {
        return None;
    }
    let scale = (unsafe { GetDpiForSystem() } as f64 / 96.0).max(1.0);
    let (free_w, free_h) = ((work.right - work.left) as f64, (work.bottom - work.top) as f64);
    let w = free_w.min((free_w * WINDOW_SIZE.0).max(WINDOW_MIN.0 * scale));
    let h = free_h.min((free_h * WINDOW_SIZE.1).max(WINDOW_MIN.1 * scale));
    let x = work.left as f64 + (free_w - w) / 2.0;
    let y = work.top as f64 + (free_h - h) / 2.0;
    Some((x as i32, y as i32, w as i32, h as i32))
}
