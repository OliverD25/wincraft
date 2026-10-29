<#
.SYNOPSIS
One picture of one WinCraft scene, now: a test instance opens the scene, its
window is captured, and the instance quits.

.DESCRIPTION
For a quick look at a UI change before a release, without the idle wait of
screenshots.ps1. The test instance (WINCRAFT_INSTANCE=sceneshot, its own
LOCALAPPDATA under %LOCALAPPDATA%\Temp, every plugin off unless -Plugins
names it) is read-only towards real windows, like every test instance.

Unlike screenshots.ps1 it does not wait for you to be away: the scene's
window shows for a few seconds and may take the keyboard while it does. The
window that was in front gets the keyboard back at the end.

The picture is taken from the screen, so glass shows what is behind it.

Exit codes: 0 saved, 1 error, 2 no window of the scene was found.

-Theme is the theme name (it goes into the test config.json). -Mode is
light or dark (it goes on the command line as --theme=<mode>).

-Demo puts a plain full-screen backdrop behind the scene, so glass themes
show nothing personal. When the scene is --open-arrange, it also opens four
demo windows (one helper program the script builds and owns), and the scene
uses their group. Everything -Demo opens is closed at the end, even after an
error. No other window is touched.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\scene-shot.ps1 -Scene '--open-palette=toggle' -Plugins quick_actions
The palette with "toggle" typed and QuickActions on.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\scene-shot.ps1 -Scene '--show-update-progress' -Theme fluent
The update panel in the Fluent theme.
#>
[CmdletBinding()]
param(
    # The scene flags, as wincraft.exe takes them (CONTRIBUTING lists them).
    [Parameter(Mandatory = $true)]
    [string[]]$Scene,
    # classic, fluent, terminal, paper or high-contrast.
    [string]$Theme = 'classic',
    # light or dark.
    [ValidateSet('light', 'dark')]
    [string]$Mode = 'dark',
    # A neutral backdrop behind the scene, and demo windows for WindowDeck.
    [switch]$Demo,
    # Plugin ids to leave on; every other plugin is off.
    [string[]]$Plugins = @(),
    # Settings to put into the test instance's config.json, as JSON.
    [string]$Config,
    [string]$Exe,
    [string]$Out,
    # How long the scene gets to draw before the picture.
    [int]$SettleMs = 4000,
    # Windows narrower than this are not the scene (tooltips, hidden helpers).
    [int]$MinWidth = 120
)

$ErrorActionPreference = 'Stop'
# With -File, a comma list arrives as one string: split it between flags.
$Scene = @($Scene | ForEach-Object { $_ -split ',(?=--)' })
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) {
    $Exe = @('target\debug\wincraft.exe', 'target\verify\release\wincraft.exe', 'target\release\wincraft.exe') |
        ForEach-Object { Join-Path $repo $_ } |
        Where-Object { Test-Path $_ } |
        Sort-Object { (Get-Item $_).LastWriteTime } -Descending |
        Select-Object -First 1
}
if (-not $Exe -or -not (Test-Path $Exe)) { Write-Output 'No wincraft.exe found: build first (cargo build).'; exit 1 }
if (-not $Out) {
    $name = ($Scene -join '_') -replace '[^A-Za-z0-9_=-]', ''
    $Out = Join-Path $repo ("target\shots\scene-{0}-{1}-{2}-{3}.png" -f $name, $Theme, $Mode, (Get-Date -Format 'HHmmss'))
}
New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;

public static class SceneShot {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint flags);
    public static IntPtr LastHandle = IntPtr.Zero;

    // Puts the scene's own window in front of the demo backdrop. It only
    // changes the order of the test instance's window, and takes no focus.
    public static void ToFront() {
        if (LastHandle == IntPtr.Zero) return;
        SetWindowPos(LastHandle, new IntPtr(-1), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010);
        SetWindowPos(LastHandle, new IntPtr(-2), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010);
    }

    // The largest visible window of the process, at least minWidth wide.
    public static int[] Largest(uint pid, int minWidth) {
        int[] best = null; long area = 0;
        EnumWindows((h, l) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            RECT r;
            if (owner == pid && IsWindowVisible(h) && GetWindowRect(h, out r)) {
                int w = r.R - r.L, hgt = r.B - r.T;
                if (w >= minWidth && (long)w * hgt > area) { area = (long)w * hgt; best = new[] { r.L, r.T, w, hgt }; LastHandle = h; }
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }

    public static void Capture(int[] rect, string path) {
        using (var bmp = new Bitmap(rect[2], rect[3]))
        using (var g = Graphics.FromImage(bmp)) {
            g.CopyFromScreen(rect[0], rect[1], 0, 0, bmp.Size);
            bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png);
        }
    }
}
'@
[SceneShot]::SetProcessDPIAware() | Out-Null

$scratch = Join-Path $env:LOCALAPPDATA 'Temp\wincraft-scene-shot'
$data = Join-Path $scratch 'WinCraft'
if (Test-Path $scratch) { Remove-Item -Recurse -Force $scratch }
New-Item -ItemType Directory -Force (Join-Path $data 'plugins') | Out-Null
$ids = (Get-Content (Join-Path $repo 'plugins.json') -Raw | ConvertFrom-Json).plugins | ForEach-Object { $_.id }
foreach ($id in $ids) {
    $on = if ($Plugins -contains $id) { 'true' } else { 'false' }
    Set-Content -Path (Join-Path $data "plugins\$id.json") -Value "{`"enabled`": $on}" -Encoding utf8
}
# A safe test config: the test copy never starts with Windows and never takes
# the palette hotkey of the real WinCraft. -Config keys win.
$settings = [ordered]@{
    start_with_windows = $false
    theme              = $Theme
    mode               = $Mode
    palette_hotkey     = 'Win+Ctrl+Alt+Shift+F23'
}
if ($Config) {
    ($Config | ConvertFrom-Json).PSObject.Properties | ForEach-Object { $settings[$_.Name] = $_.Value }
}
Set-Content -Path (Join-Path $data 'config.json') -Value ($settings | ConvertTo-Json -Depth 8) -Encoding utf8

$front = [SceneShot]::GetForegroundWindow()
$demoProcess = $null
$process = $null
$code = 0
try {

if ($Demo) {
    $hostDir = Join-Path $env:LOCALAPPDATA 'Temp\wincraft-scene-demo'
    if (Test-Path $hostDir) { Remove-Item -Recurse -Force $hostDir }
    New-Item -ItemType Directory -Force $hostDir | Out-Null
    $hostExe = Join-Path $hostDir 'Notes.exe'
    Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing -OutputType WindowsApplication -OutputAssembly $hostExe -TypeDefinition @'
using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

class QuietForm : Form {
    public bool Hidden;
    protected override bool ShowWithoutActivation { get { return true; } }
    protected override CreateParams CreateParams {
        get {
            var cp = base.CreateParams;
            if (Hidden) { cp.ExStyle |= 0x80; cp.ExStyle &= ~0x40000; }
            return cp;
        }
    }
}

static class DemoHost {
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [STAThread]
    static void Main(string[] a) {
        string dir = a[0]; bool dark = a[1] == "dark"; bool windows = a[2] == "windows";
        var area = Screen.PrimaryScreen.Bounds;
        var forms = new System.Collections.Generic.List<Form>();
        var back = new QuietForm { Text = "Desktop notes", Hidden = true, FormBorderStyle = FormBorderStyle.None, StartPosition = FormStartPosition.Manual,
            Bounds = area, ShowInTaskbar = false, BackColor = dark ? Color.FromArgb(20, 22, 28) : Color.FromArgb(226, 228, 233) };
        string[] titles = { "Inbox - Mail", "Research notes", "Build log", "Budget 2026" };
        string[] lines = {
            "Inbox\n\n3 new messages\nWeekly plan\nInvoice 1042\nLunch on Friday",
            "Research notes\n\nIdeas for the next sprint\n- faster search\n- fewer clicks\n- clearer labels",
            "Build log\n\n[ok] compile\n[ok] test 214 passed\n[ok] package ready",
            "Budget 2026\n\nTravel      1 200\nSoftware      480\nTraining      900\nTotal       2 580" };
        var accent = new[] { Color.FromArgb(0, 103, 192), Color.FromArgb(16, 124, 16), Color.FromArgb(202, 80, 16), Color.FromArgb(135, 100, 184) };
        if (windows) {
            for (int i = 0; i < 4; i++) {
                var f = new QuietForm { Text = titles[i], StartPosition = FormStartPosition.Manual,
                    Bounds = new Rectangle(area.Left + 180 + i * 60, area.Top + 140 + i * 50, 640, 420),
                    BackColor = Color.White, ShowInTaskbar = true };
                f.Controls.Add(new Label { Dock = DockStyle.Fill, Text = lines[i], ForeColor = Color.FromArgb(32, 32, 32),
                    Font = new Font("Segoe UI", 22f), Padding = new Padding(28, 20, 10, 10) });
                f.Controls.Add(new Panel { Dock = DockStyle.Top, Height = 14, BackColor = accent[i] });
                forms.Add(f);
            }
        }
        var timer = new Timer { Interval = 200 };
        timer.Tick += (s, e) => { if (File.Exists(Path.Combine(dir, "quit.txt"))) Application.Exit(); };
        back.Shown += (s, e) => {
            foreach (var f in forms) f.Show();
            // The demo window in front is what the scene sees as "the window in front".
            keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero);
            uint pid; uint other = GetWindowThreadProcessId(GetForegroundWindow(), out pid); uint mine = GetCurrentThreadId();
            AttachThreadInput(mine, other, true);
            SetForegroundWindow(forms.Count > 0 ? forms[forms.Count - 1].Handle : back.Handle);
            AttachThreadInput(mine, other, false);
            File.WriteAllText(Path.Combine(dir, "ready.txt"), "ready");
            timer.Start();
        };
        Application.Run(back);
    }
}
'@
    $withWindows = [bool]($Scene | Where-Object { $_ -like '--open-arrange=*' })
    $demoProcess = Start-Process -FilePath $hostExe -PassThru -ArgumentList @("`"$hostDir`"", $Mode, $(if ($withWindows) { 'windows' } else { 'none' }))
    $ready = Join-Path $hostDir 'ready.txt'
    $deadline = (Get-Date).AddSeconds(15)
    while (-not (Test-Path $ready) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path $ready)) { Stop-Process -Id $demoProcess.Id -Force -ErrorAction SilentlyContinue; throw 'the demo backdrop did not appear within 15 s' }
    $group = $hostExe.ToLower()
    $Scene = @($Scene | ForEach-Object { $_.Replace('{demo-group}', $group) })
    $Scene = @($Scene | ForEach-Object { if ($_ -eq '--open-arrange=demo') { "--open-arrange=$group" } else { $_ } })
}

$env:WINCRAFT_INSTANCE = 'sceneshot'
$env:LOCALAPPDATA = $scratch
$process = Start-Process -FilePath $Exe -ArgumentList (@("--theme=$Mode") + $Scene | ForEach-Object { if ($_ -match '\s') { "`"$_`"" } else { $_ } }) -PassThru
try {
    Start-Sleep -Milliseconds $SettleMs
    $rect = [SceneShot]::Largest([uint32]$process.Id, $MinWidth)
    if ($rect) {
        if ($Demo) { [SceneShot]::ToFront(); Start-Sleep -Milliseconds 500 }
        [SceneShot]::Capture($rect, $Out)
        Write-Output ("saved {0} ({1}x{2})" -f $Out, $rect[2], $rect[3])
    } else {
        Write-Output 'No window of the scene showed up. Is the scene flag right, and is its plugin in -Plugins?'
        $code = 2
    }
} finally {
    & $Exe --quit | Out-Null
}
} finally {
    if ($demoProcess) {
        [IO.File]::WriteAllText((Join-Path $hostDir 'quit.txt'), 'quit')
        if (-not $demoProcess.WaitForExit(5000)) { Stop-Process -Id $demoProcess.Id -Force -ErrorAction SilentlyContinue }
    }
    if ($front -ne [IntPtr]::Zero) { [SceneShot]::SetForegroundWindow($front) | Out-Null }
}
exit $code
