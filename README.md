<p align="center">
  <img src="brand/thumbnail-logo.svg" alt="Thumbnail logo" width="128" height="128">
</p>

<h1 align="center">Thumbnail</h1>

<p align="center">
  <b>Your own video library, as a clean thumbnail grid.</b><br>
  A light, portable Windows app that turns the videos on your drives into a home page you can browse.<br>
  Add a folder or a single video, and it shows up as a card. Click a card and it opens in your default player.
</p>

<p align="center">
  <img alt="Python" src="https://img.shields.io/badge/Python-standard%20library%20only-3776AB?logo=python&logoColor=white">
  <img alt="Platform" src="https://img.shields.io/badge/Windows-10%20%C2%B7%2011-0078D4?logo=windows&logoColor=white">
  <img alt="Portable" src="https://img.shields.io/badge/portable-no%20installer-e8352b">
  <img alt="License" src="https://img.shields.io/badge/license-all%20rights%20reserved-555555">
</p>

---

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
