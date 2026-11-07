// Puts the icon and the name into Thumbnail.exe (Explorer, taskbar, Alt+Tab).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("brand/icon.ico");
        res.set("ProductName", "Thumbnail");
        res.set("FileDescription", "Thumbnail");
        res.set("OriginalFilename", "Thumbnail.exe");
        res.compile().expect("could not add the icon to the exe");
    }
}
