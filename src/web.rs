//! The page (HTML, CSS, JS, fonts), built into the exe. The files themselves live in app\web.

macro_rules! files {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../app/web/", $name)) as &[u8])),*]
    };
}

static FILES: &[(&str, &[u8])] = files![
    "index.html",
    "app.js",
    "style.css",
    "logo.svg",
    "fonts/Archivo-Variable.woff2",
    "fonts/Fraunces-Variable.woff2",
    "fonts/VT323-Regular.woff2",
];

pub fn file(path: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(name, _)| *name == path).map(|(_, data)| *data)
}
