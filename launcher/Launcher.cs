// Thumbnail.exe: starts the portable app (runtime\pythonw.exe app\app.py) from its own folder.
using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;

[assembly: AssemblyTitle("Thumbnail")]
[assembly: AssemblyProduct("Thumbnail")]
[assembly: AssemblyVersion("1.0.0.0")]

static class Launcher
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int MessageBoxW(IntPtr owner, string text, string caption, uint type);

    static int Main()
    {
        string dir = AppDomain.CurrentDomain.BaseDirectory;
        string python = Path.Combine(dir, @"runtime\pythonw.exe");
        string app = Path.Combine(dir, @"app\app.py");
        if (!File.Exists(python) || !File.Exists(app))
        {
            MessageBoxW(IntPtr.Zero, "Some files are missing next to Thumbnail.exe.\n\n" +
                "Keep the app, runtime and data folders together with it.", "Thumbnail", 0x10);
            return 1;
        }
        var start = new ProcessStartInfo(python, "\"" + app + "\"") { WorkingDirectory = dir, UseShellExecute = false };
        Process.Start(start);
        return 0;
    }
}
