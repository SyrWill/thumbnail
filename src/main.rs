//! Thumbnail - a home page of thumbnails for the videos on your drives.
//!
//! One exe: a small local server (the library and its JSON API) plus a native window
//! (WebView2) that shows the page. The portable folder looks like this:
//!
//!     Thumbnail.exe        this program (the page is built in)
//!     data\                library, covers, channel photos, collections, settings

#![windows_subsystem = "windows"]

mod dialogs;
mod library;
mod paths;
mod platform;
mod server;
mod web;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tao::dpi::{PhysicalPosition, PhysicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Fullscreen, Icon, ResizeDirection, Theme, WindowBuilder};
use wry::{WebContext, WebViewBuilder};

const TITLE: &str = "Thumbnail";

/// Full screen from the page (its button, or F11) makes the whole window full screen,
/// like it was in the browser window.
const INIT_SCRIPT: &str = r#"
(() => {
  const post = (m) => window.ipc && window.ipc.postMessage(m);
  document.addEventListener("fullscreenchange", () => post(document.fullscreenElement ? "fullscreen:on" : "fullscreen:off"));
  addEventListener("keydown", (e) => {
    if (e.key !== "F11") return;
    e.preventDefault();
    if (document.fullscreenElement) document.exitFullscreen();
    else document.documentElement.requestFullscreen().catch(() => {});
  }, true);
})();
"#;

enum UserEvent {
    Fullscreen(bool),
    Window(String), // minimize, maximize, close, drag, ready, resize-n / -ne / -nw
}

fn tell_maximized(webview: &wry::WebView, on: bool) {
    let _ = webview.evaluate_script(&format!("window.__setMaximized && window.__setMaximized({on})"));
}

fn main() {
    let exe = std::env::current_exe().unwrap_or_default();
    let root = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    library::init_dirs(&root);
    let d = library::dirs();
    let _ = std::fs::create_dir_all(&d.covers);
    let _ = std::fs::create_dir_all(&d.avatars);

    // One copy at a time: a second start just brings the open window to the front.
    let addr = format!("{}:{}", server::HOST, server::PORT);
    let http = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(_) => {
            if already_running() {
                platform::focus_window(TITLE);
            } else {
                platform::error_box(&format!(
                    "Thumbnail couldn't start: another program is using port {}.",
                    server::PORT
                ));
            }
            return;
        }
    };

    let lib = library::Library::load();
    lib.prune_files();
    server::init(lib);
    // Drive letters can change (external drive on another PC): look for missing videos elsewhere.
    std::thread::spawn(|| {
        server::lib().reconnect_drives();
    });
    std::thread::spawn(move || server::serve(http));

    if let Err(e) = run_window() {
        platform::error_box(&format!("Thumbnail couldn't open its window.\n\n{e}"));
    }
}

/// The open copy answers our ping.
fn already_running() -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(
        &format!("{}:{}", server::HOST, server::PORT).parse().expect("address"),
        Duration::from_secs(2),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!("GET /api/ping HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n", server::HOST, server::PORT);
    let mut answer = String::new();
    s.write_all(req.as_bytes()).is_ok() && s.read_to_string(&mut answer).is_ok() && answer.contains("video-library")
}

fn run_window() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // The exe's own icon at full size: sharp in the taskbar on big and scaled screens.
    let icon = |px: u32| Icon::from_resource(1, Some(PhysicalSize::new(px, px))).ok();
    // No Windows title bar: the page's top bar is the title bar (move, maximize, close),
    // like Spotify or Discord. The shadow keeps the window's edges and its resize borders.
    let mut builder = WindowBuilder::new()
        .with_title(TITLE)
        .with_visible(false)
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_min_inner_size(PhysicalSize::new(720u32, 480u32))
        .with_theme(Some(Theme::Dark))
        .with_window_icon(icon(32))
        .with_taskbar_icon(icon(256));
    if let Some((x, y, w, h)) = platform::startup_bounds() {
        builder = builder.with_position(PhysicalPosition::new(x, y)).with_inner_size(PhysicalSize::new(w as u32, h as u32));
    }
    let window = builder.build(&event_loop)?;
    dialogs::OWNER.store(window.hwnd(), std::sync::atomic::Ordering::Relaxed);

    // The window's browser files (caches etc.) stay on each PC, outside the portable folder.
    let local = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("TEMP")).unwrap_or_default();
    let mut context = WebContext::new(Some(std::path::PathBuf::from(local).join("Thumbnail").join("webview")));
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(format!("http://{}:{}/", server::HOST, server::PORT))
        .with_initialization_script(INIT_SCRIPT)
        .with_background_color((15, 15, 15, 255))
        .with_hotkeys_zoom(true)
        .with_ipc_handler(move |req| {
            let event = match req.body().as_str() {
                "fullscreen:on" => UserEvent::Fullscreen(true),
                "fullscreen:off" => UserEvent::Fullscreen(false),
                other => match other.strip_prefix("window:") {
                    Some(action) => UserEvent::Window(action.to_string()),
                    None => return,
                },
            };
            let _ = proxy.send_event(event);
        })
        .build(&window)?;

    // Outer size is only known now: centre the whole window, frame included.
    if let Some((x, y, w, h)) = platform::startup_bounds() {
        let outer = window.outer_size();
        let inner = window.inner_size();
        let (fw, fh) = (outer.width.saturating_sub(inner.width) as i32, outer.height.saturating_sub(inner.height) as i32);
        window.set_inner_size(PhysicalSize::new((w - fw).max(400) as u32, (h - fh).max(300) as u32));
        window.set_outer_position(PhysicalPosition::new(x, y));
    }
    window.set_visible(true);
    let mut last_maximized = window.is_maximized();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_alive = (&webview, &context);
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Fullscreen(on)) => {
                window.set_fullscreen(if on { Some(Fullscreen::Borderless(None)) } else { None });
            }
            // The page's own title bar buttons and drag areas.
            Event::UserEvent(UserEvent::Window(action)) => match action.as_str() {
                "minimize" => window.set_minimized(true),
                "maximize" => window.set_maximized(!window.is_maximized()),
                "close" => *control_flow = ControlFlow::Exit,
                "drag" => {
                    let _ = window.drag_window();
                }
                "ready" => tell_maximized(&webview, window.is_maximized()),
                edge => {
                    let dir = match edge {
                        "resize-n" => ResizeDirection::North,
                        "resize-ne" => ResizeDirection::NorthEast,
                        "resize-nw" => ResizeDirection::NorthWest,
                        _ => return,
                    };
                    let _ = window.drag_resize_window(dir);
                }
            },
            // Maximize / restore from anywhere (buttons, double-click, Win+Up, snapping):
            // the page shows the matching icon.
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                let max = window.is_maximized();
                if max != last_maximized {
                    last_maximized = max;
                    tell_maximized(&webview, max);
                }
            }
            _ => {}
        }
    });
}
