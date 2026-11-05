# Thumbnail

A light, portable Windows app that turns the videos on your drives into a clean **thumbnail grid** — like a home page for your own video library.

Add a folder or a single video, and Thumbnail shows it as a card. Click a card and the video opens in your default player.

## Features

- Thumbnail grid with an adjustable size
- Channels, collections and multi-select to organize videos
- Several color themes, including a retro one
- Search across the whole library
- Videos play in your default player; nothing is uploaded anywhere
- Portable: one folder, no installer, no account, no internet needed

## How it works

A small local server written with the **Python standard library only** (no pip packages) serves the page on `127.0.0.1` and opens it in a Microsoft Edge app-mode window. Your library lives in a `data\` folder next to the app.

```
Thumbnail.exe     tiny C# launcher: starts runtime\pythonw.exe app\app.py
app\              app.py (server) and web\ (HTML, CSS, JS, fonts)
runtime\          a minimal Python (not in this repo)
data\             your library, covers, collections, settings (not in this repo)
launcher\         source and icon of Thumbnail.exe
brand\            logo
```

## Run it from source

You need Windows 10/11 (for Microsoft Edge) and any Python 3.

```bash
python app/app.py
```

Use `python app/app.py --no-window` to start only the server.

## Build the portable folder

1. Take the Windows *embeddable package* from python.org and put it in a `runtime\` folder.
2. Compile the launcher with the .NET Framework compiler that ships with Windows:

   ```bash
   C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /target:winexe /win32icon:launcher\icon.ico /out:Thumbnail.exe launcher\Launcher.cs
   ```

3. Keep `Thumbnail.exe`, `app\`, `runtime\` and `THIRD-PARTY-NOTICES.txt` together. The `data\` folder is created on first run.

## Third-party software

Fonts and the Python runtime have their own licenses. See [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

## License

Copyright © 2026. All rights reserved. The source is published for viewing and as a portfolio piece; it may not be copied, modified, redistributed or sold without permission.
