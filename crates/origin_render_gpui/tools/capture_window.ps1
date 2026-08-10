# Capture the client area of a window by exact title to a PNG.
#
# Used by `examples/parity_capture.rs` to obtain the pixels official GPUI actually rasterized, so
# they can be diffed against Origin's existing backends.
#
# Why PrintWindow rather than BitBlt from the window DC: GPUI presents a hardware-composed
# D3D11/DXGI swapchain, and a plain BitBlt of such a window usually yields black. PrintWindow with
# PW_RENDERFULLCONTENT (0x2) asks DWM for the composed window content, which works for
# hardware-composed surfaces and is unaffected by occlusion or the cursor.
#
# All P/Invoke lives here, in .NET, so no `unsafe` Rust enters the repository (the workspace sets
# `unsafe_code = "forbid"`).
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File capture_window.ps1 -Title "<exact title>" -Out out.png
# Prints one line on success:  OK <width> <height>
# Prints ERR <reason> and exits non-zero otherwise.

param(
    [Parameter(Mandatory = $true)][string]$Title,
    [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing

$source = @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class WinCap
{
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr FindWindowW(string cls, string title);

    [DllImport("user32.dll")]
    static extern bool GetWindowRect(IntPtr hWnd, out RECT r);

    [DllImport("user32.dll")]
    static extern bool GetClientRect(IntPtr hWnd, out RECT r);

    [DllImport("user32.dll")]
    static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);

    [DllImport("user32.dll")]
    static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);

    [DllImport("user32.dll", SetLastError = true)]
    static extern bool SetProcessDpiAwarenessContext(IntPtr value);

    [DllImport("user32.dll")]
    static extern bool SetProcessDPIAware();

    const uint PW_RENDERFULLCONTENT = 0x2;

    // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
    static readonly IntPtr PER_MONITOR_AWARE_V2 = new IntPtr(-4);

    /// Windows virtualizes window rectangles for DPI-unaware callers: a 720x450 physical client area
    /// is reported as 480x300 to a process that has not declared awareness, and PrintWindow then
    /// renders at that virtualized size. Declaring awareness before any query is what makes the
    /// capture come back at true device resolution. Must run before the first rect query.
    public static void MakeDpiAware()
    {
        try { if (SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)) return; } catch { }
        try { SetProcessDPIAware(); } catch { }
    }

    // Returns "w h" of the saved client-area image.
    public static string Capture(string title, string outPath)
    {
        IntPtr h = FindWindowW(null, title);
        if (h == IntPtr.Zero) throw new Exception("window-not-found");

        RECT wr, cr;
        if (!GetWindowRect(h, out wr)) throw new Exception("GetWindowRect-failed");
        if (!GetClientRect(h, out cr)) throw new Exception("GetClientRect-failed");

        int winW = wr.Right - wr.Left, winH = wr.Bottom - wr.Top;
        int cliW = cr.Right - cr.Left, cliH = cr.Bottom - cr.Top;
        if (winW <= 0 || winH <= 0 || cliW <= 0 || cliH <= 0) throw new Exception("degenerate-rect");

        // Where the client area starts inside the window, in device pixels.
        POINT origin = new POINT();
        origin.X = 0; origin.Y = 0;
        if (!ClientToScreen(h, ref origin)) throw new Exception("ClientToScreen-failed");
        int offX = origin.X - wr.Left, offY = origin.Y - wr.Top;

        using (Bitmap full = new Bitmap(winW, winH, PixelFormat.Format32bppArgb))
        {
            using (Graphics g = Graphics.FromImage(full))
            {
                IntPtr hdc = g.GetHdc();
                bool ok;
                try { ok = PrintWindow(h, hdc, PW_RENDERFULLCONTENT); }
                finally { g.ReleaseHdc(hdc); }
                if (!ok) throw new Exception("PrintWindow-failed");
            }

            // Crop to the client area only, so the captured image is exactly the surface the
            // adapter painted into.
            Rectangle crop = new Rectangle(offX, offY, cliW, cliH);
            crop.Intersect(new Rectangle(0, 0, winW, winH));
            if (crop.Width <= 0 || crop.Height <= 0) throw new Exception("crop-empty");

            using (Bitmap client = full.Clone(crop, PixelFormat.Format32bppArgb))
            {
                client.Save(outPath, ImageFormat.Png);
                return client.Width + " " + client.Height;
            }
        }
    }
}
'@

try {
    Add-Type -TypeDefinition $source -ReferencedAssemblies System.Drawing, System.Windows.Forms | Out-Null
    [WinCap]::MakeDpiAware()
    $size = [WinCap]::Capture($Title, $Out)
    Write-Output "OK $size"
    exit 0
}
catch {
    Write-Output "ERR $($_.Exception.Message)"
    exit 1
}
