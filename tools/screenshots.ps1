<#
.SYNOPSIS
Screenshots of every WinCraft scene, taken from a test instance while the
user is away from the PC, stopped the moment they come back.

.DESCRIPTION
Each scene is a fresh test instance (WINCRAFT_INSTANCE=shot, its own
LOCALAPPDATA under the output folder) started with a scene flag, captured,
and stopped with --quit. A test instance is read-only towards real windows,
and the harness sends no input at all.

The run refuses to start unless the user has been idle for -IdleSeconds and
the desktop is unlocked. Before and after every scene it checks the last
input time again; any input since the start aborts the run at once: the test
instance is stopped, the foreground window is given back, ABORTED.txt names
the scene reached, and the exit code is 3.

Exit codes: 0 done, 1 error, 2 refused to start, 3 aborted because the user
came back.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\screenshots.ps1 -DryRun
#>
[CmdletBinding()]
param(
    [int]$IdleSeconds = 180,
    [string]$Exe,
    [string]$Out,
    [switch]$DryRun,
    # Test switch: behave as if the user touched the PC after scene N.
    [int]$SimulateInputAfterScene = 0
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\verify\release\wincraft.exe' }
if (-not $Out) { $Out = Join-Path $repo ('target\shots\' + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
$Instance = 'shot'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;

public static class Shot {
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    delegate bool EnumProc(IntPtr hwnd, IntPtr data);

    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO info);
    [DllImport("kernel32.dll")] static extern uint GetTickCount();
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetUserObjectInformationW(IntPtr obj, int index, StringBuilder info, int length, out int needed);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr hwnd, StringBuilder text, int length);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr hwnd, StringBuilder text, int length);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attribute, out RECT rect, int size);

    static readonly IntPtr PerMonitorAware2 = new IntPtr(-4);

    /// Milliseconds on the tick counter at the last keyboard or mouse input.
    public static uint LastInput() {
        LASTINPUTINFO info = new LASTINPUTINFO();
        info.cbSize = (uint)Marshal.SizeOf(info);
        GetLastInputInfo(ref info);
        return info.dwTime;
    }

    public static double IdleSeconds() {
        return unchecked(GetTickCount() - LastInput()) / 1000.0;
    }

    /// "Default" while the desktop is unlocked; the lock screen and UAC run
    /// on other desktops, which a normal process cannot even open.
    public static string InputDesktop() {
        IntPtr desktop = OpenInputDesktop(0, false, 0x0001);
        if (desktop == IntPtr.Zero) { return ""; }
        StringBuilder name = new StringBuilder(256);
        int needed;
        GetUserObjectInformationW(desktop, 2, name, name.Capacity * 2, out needed);
        CloseDesktop(desktop);
        return name.ToString();
    }

    public class Found {
        public IntPtr Handle; public string Title; public string Class; public RECT Rect;
        public int Width { get { return Rect.Right - Rect.Left; } }
        public int Height { get { return Rect.Bottom - Rect.Top; } }
    }

    /// The visible top-level windows of one process, in physical pixels,
    /// without the invisible resize borders.
    public static List<Found> WindowsOf(uint pid) {
        IntPtr previous = SetThreadDpiAwarenessContext(PerMonitorAware2);
        List<Found> found = new List<Found>();
        EnumWindows(delegate (IntPtr hwnd, IntPtr data) {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            if (owner != pid || !IsWindowVisible(hwnd)) { return true; }
            StringBuilder title = new StringBuilder(256);
            GetWindowTextW(hwnd, title, title.Capacity);
            StringBuilder cls = new StringBuilder(256);
            GetClassNameW(hwnd, cls, cls.Capacity);
            RECT rect;
            if (DwmGetWindowAttribute(hwnd, 9, out rect, Marshal.SizeOf(typeof(RECT))) != 0) {
                GetWindowRect(hwnd, out rect);
            }
            Found item = new Found();
            item.Handle = hwnd; item.Title = title.ToString(); item.Class = cls.ToString(); item.Rect = rect;
            if (item.Width > 0 && item.Height > 0) { found.Add(item); }
            return true;
        }, IntPtr.Zero);
        SetThreadDpiAwarenessContext(previous);
        return found;
    }

    public static void Capture(RECT rect, string path) {
        IntPtr previous = SetThreadDpiAwarenessContext(PerMonitorAware2);
        try {
            int width = rect.Right - rect.Left, height = rect.Bottom - rect.Top;
            using (Bitmap bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb))
            using (Graphics graphics = Graphics.FromImage(bitmap)) {
                graphics.CopyFromScreen(rect.Left, rect.Top, 0, 0, new Size(width, height), CopyPixelOperation.SourceCopy);
                bitmap.Save(path, ImageFormat.Png);
            }
        } finally {
            SetThreadDpiAwarenessContext(previous);
        }
    }
}
'@

# ---------------------------------------------------------------- scenes

$scenes = @(
    @{ Name = 'palette-empty';          Args = @('--open-palette');                        Window = 'palette' },
    @{ Name = 'palette-paths';          Args = @('--open-palette=/');                      Window = 'palette'; Settle = 1500 },
    @{ Name = 'palette-calc';           Args = @('--open-palette==2+2*3');                 Window = 'palette' },
    @{ Name = 'palette-windows';        Args = @('--open-palette=<');                      Window = 'palette'; Settle = 1200 },
    @{ Name = 'palette-help';           Args = @('--open-palette=?');                      Window = 'palette' },
    @{ Name = 'settings-general';       Args = @('--open-settings=general');               Window = 'settings' },
    @{ Name = 'settings-plugins';       Args = @('--open-settings=plugins');               Window = 'settings' },
    @{ Name = 'settings-layout-keeper'; Args = @('--open-settings=plugin:layout_keeper');  Window = 'settings' },
    @{ Name = 'settings-about';         Args = @('--open-settings=about');                 Window = 'settings' },
    @{ Name = 'strip';                  Args = @('--open-arrange=0');                      Window = 'strip';    Settle = 1200 },
    @{ Name = 'strip-peek';             Args = @('--open-arrange=0', '--peek-card=chip');  Window = 'peek';     Settle = 1500 }
)
$themes = @('dark', 'light')

function Write-Plan {
    $n = 0
    foreach ($theme in $themes) {
        foreach ($scene in $scenes) {
            $n++
            '{0:D2}_{1}_{2}.png  <- --theme={2} {3}' -f $n, $scene.Name, $theme, ($scene.Args -join ' ')
        }
    }
}

# ---------------------------------------------------------------- checks

function Get-OtherWinCraft {
    @(Get-Process wincraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne $Exe } |
        ForEach-Object { '{0} {1}' -f $_.Id, $_.Path })
}

$problems = @()
$idle = [Shot]::IdleSeconds()
$desktop = [Shot]::InputDesktop()
if ($idle -lt $IdleSeconds) { $problems += ('the user was active {0:N0} s ago; the run needs {1} s of idle' -f $idle, $IdleSeconds) }
if ($desktop -ne 'Default') { $problems += ('the input desktop is "{0}", not Default: locked or on a secure screen' -f $desktop) }
if (-not (Test-Path -LiteralPath $Exe)) { $problems += "no build at $Exe" }
$running = Get-Process wincraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe }
if ($running) { $problems += "a WinCraft from $Exe is already running (PID $($running.Id -join ', ')); the harness only runs an exe nobody else is running" }

"WinCraft screenshot run"
"  exe:      $Exe"
"  output:   $Out"
"  idle:     {0:N0} s (needs {1})" -f $idle, $IdleSeconds
"  desktop:  $desktop"
"  instance: WINCRAFT_INSTANCE=$Instance"
"  other WinCraft processes (never touched): $((Get-OtherWinCraft) -join '; ')"
"  plan:"
Write-Plan | ForEach-Object { "    $_" }

if ($problems.Count -gt 0) {
    "REFUSED:"
    $problems | ForEach-Object { "  - $_" }
    exit 2
}
if ($DryRun) {
    "Dry run: every condition holds; nothing was started."
    exit 0
}

# ---------------------------------------------------------------- run

$appdata = Join-Path $Out 'appdata'
$plugins = Join-Path $appdata 'WinCraft\plugins'
New-Item -ItemType Directory -Force -Path $plugins | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $appdata 'WinCraft\config.json'),
    '{"start_with_windows": false, "theme": "dark", "palette_hotkey": "Win+Ctrl+Alt+Shift+F23"}', $utf8)
foreach ($id in 'language_indicator', 'screen_dimmer', 'shortcut_detector') {
    [IO.File]::WriteAllText((Join-Path $plugins "$id.json"), '{"enabled": false}', $utf8)
}
[IO.File]::WriteAllText((Join-Path $plugins 'layout_keeper.json'),
    '{"enabled": true, "settings": {"restore_on_start": false, "snapshot_interval_seconds": 600}}', $utf8)

$saved = @{ Instance = $env:WINCRAFT_INSTANCE; AppData = $env:LOCALAPPDATA }
$env:WINCRAFT_INSTANCE = $Instance
$env:LOCALAPPDATA = $appdata

$report = New-Object System.Collections.Generic.List[string]
$report.Add("WinCraft screenshot run, started $(Get-Date -Format s)")
$report.Add("exe: $Exe")
$others = Get-OtherWinCraft
$report.Add("other WinCraft processes at the start: $($others -join '; ')")
$foreground = [Shot]::GetForegroundWindow()
$startInput = [Shot]::LastInput()
$runStart = Get-Date
$current = $null
$reached = 'none'
$exitCode = 0

class UserCameBack : System.Exception {
    UserCameBack([string]$message) : base($message) {}
}

function Test-StillAway([string]$when, [int]$sceneNumber) {
    if ([Shot]::LastInput() -ne $startInput) {
        throw [UserCameBack]::new("input $when")
    }
    if ([Shot]::InputDesktop() -ne 'Default') {
        throw [UserCameBack]::new("the desktop was locked $when")
    }
    if ($SimulateInputAfterScene -gt 0 -and $sceneNumber -ge $SimulateInputAfterScene -and $when -like 'after*') {
        throw [UserCameBack]::new("simulated input $when (-SimulateInputAfterScene $SimulateInputAfterScene)")
    }
}

function Stop-TestInstance {
    $quit = Start-Process -FilePath $Exe -ArgumentList '--quit' -PassThru -Wait -WindowStyle Hidden
    if ($script:current -and -not $script:current.HasExited) {
        if (-not $script:current.WaitForExit(5000)) {
            $report.Add("  --quit did not stop PID $($script:current.Id) within 5 s; killed")
            Stop-Process -Id $script:current.Id -Force
        }
    }
    $script:current = $null
    return $quit.ExitCode
}

function Find-Target([System.Diagnostics.Process]$process, [string]$kind) {
    $windows = [Shot]::WindowsOf([uint32]$process.Id)
    switch ($kind) {
        'palette'  { $windows | Where-Object { $_.Title -eq 'WinCraft' -and $_.Class -notlike 'WinCraftHost*' } | Select-Object -First 1 }
        'settings' { $windows | Where-Object { $_.Title -eq 'WinCraft' -and $_.Class -notlike 'WinCraftHost*' } |
                         Sort-Object { $_.Width * $_.Height } -Descending | Select-Object -First 1 }
        'strip'    { $windows | Where-Object { $_.Title -eq 'WinCraft Arrange' } | Select-Object -First 1 }
        'peek'     { $windows | Where-Object { $_.Class -eq 'WinCraftPeek' } |
                         Sort-Object { $_.Width * $_.Height } -Descending | Select-Object -First 1 }
    }
}

try {
    $n = 0
    foreach ($theme in $themes) {
        foreach ($scene in $scenes) {
            $n++
            $file = '{0:D2}_{1}_{2}.png' -f $n, $scene.Name, $theme
            $reached = "$n ($($scene.Name), $theme)"
            Test-StillAway "before scene $n" $n
            $began = Get-Date
            $arguments = @("--theme=$theme") + $scene.Args
            $script:current = Start-Process -FilePath $Exe -ArgumentList $arguments -PassThru
            $target = $null
            $deadline = (Get-Date).AddSeconds(10)
            # A window is first shown where Windows puts it and moved to its
            # place a few hundred ms later, so wait until it stops moving.
            $steady = 0
            $lastRect = ''
            while ($steady -lt 6 -and (Get-Date) -lt $deadline) {
                Start-Sleep -Milliseconds 100
                $target = Find-Target $script:current $scene.Window
                $rect = if ($target) { '{0},{1},{2},{3}' -f $target.Rect.Left, $target.Rect.Top, $target.Width, $target.Height } else { '' }
                if ($rect -and $rect -eq $lastRect) { $steady++ } else { $steady = 0 }
                $lastRect = $rect
            }
            if ($target) {
                $settle = 700
                if ($scene.Settle) { $settle = $scene.Settle }
                Start-Sleep -Milliseconds $settle
                $target = Find-Target $script:current $scene.Window
            }
            if ($target) {
                [Shot]::Capture($target.Rect, (Join-Path $Out $file))
                $line = '{0}  {1}x{2} at ({3},{4})' -f $file, $target.Width, $target.Height, $target.Rect.Left, $target.Rect.Top
            } else {
                $line = "$file  FAILED: no $($scene.Window) window appeared within 10 s"
            }
            $quitCode = Stop-TestInstance
            $report.Add(('{0}  {1:N1} s, --quit exit {2}' -f $line, ((Get-Date) - $began).TotalSeconds, $quitCode))
            Test-StillAway "after scene $n" $n
        }
    }
    $report.Add("all scenes done")
}
catch [UserCameBack] {
    $exitCode = 3
    $message = $_.Exception.Message
    $report.Add("ABORTED at scene $reached`: $message")
    [IO.File]::WriteAllText((Join-Path $Out 'ABORTED.txt'),
        "Aborted at scene $reached because of $message, $(Get-Date -Format s).`r`n", $utf8)
}
catch {
    $exitCode = 1
    $report.Add("ERROR at scene $reached`: $($_.Exception.Message)")
}
finally {
    if ($script:current) { [void](Stop-TestInstance) }
    $left = @(Get-Process wincraft -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $Exe -and $_.StartTime -ge $runStart })
    foreach ($process in $left) {
        $report.Add("test instance PID $($process.Id) was still running at the end; killed")
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    $leftNow = @(Get-Process wincraft -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe })
    $report.Add("test processes left: $($leftNow.Count)")
    if ([Shot]::IsWindow($foreground)) { [void][Shot]::SetForegroundWindow($foreground) }
    $report.Add("foreground given back: $([Shot]::GetForegroundWindow() -eq $foreground)")
    $othersAfter = Get-OtherWinCraft
    if (($othersAfter -join ';') -ne ($others -join ';')) {
        $report.Add("WARNING: other WinCraft processes changed during the run: before [$($others -join '; ')], after [$($othersAfter -join '; ')]")
    } else {
        $report.Add("other WinCraft processes unchanged: $($othersAfter -join '; ')")
    }
    $env:WINCRAFT_INSTANCE = $saved.Instance
    $env:LOCALAPPDATA = $saved.AppData
    $report.Add("finished $(Get-Date -Format s), exit code $exitCode")
    [IO.File]::WriteAllLines((Join-Path $Out 'report.txt'), $report, $utf8)
    $report | ForEach-Object { $_ }
}
exit $exitCode
