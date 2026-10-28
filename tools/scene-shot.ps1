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
    $Out = Join-Path $repo ("target\shots\scene-{0}-{1}-{2}.png" -f $name, $Theme, (Get-Date -Format 'HHmmss'))
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

    // The largest visible window of the process, at least minWidth wide.
    public static int[] Largest(uint pid, int minWidth) {
        int[] best = null; long area = 0;
        EnumWindows((h, l) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            RECT r;
            if (owner == pid && IsWindowVisible(h) && GetWindowRect(h, out r)) {
                int w = r.R - r.L, hgt = r.B - r.T;
                if (w >= minWidth && (long)w * hgt > area) { area = (long)w * hgt; best = new[] { r.L, r.T, w, hgt }; }
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
New-Item -ItemType Directory -Force (Join-Path $data 'plugins') | Out-Null
$ids = (Get-Content (Join-Path $repo 'plugins.json') -Raw | ConvertFrom-Json).plugins | ForEach-Object { $_.id }
foreach ($id in $ids) {
    $on = if ($Plugins -contains $id) { 'true' } else { 'false' }
    Set-Content -Path (Join-Path $data "plugins\$id.json") -Value "{`"enabled`": $on}" -Encoding utf8
}
if ($Config) { Set-Content -Path (Join-Path $data 'config.json') -Value $Config -Encoding utf8 }

$front = [SceneShot]::GetForegroundWindow()
$env:WINCRAFT_INSTANCE = 'sceneshot'
$env:LOCALAPPDATA = $scratch
$code = 0
$process = Start-Process -FilePath $Exe -ArgumentList (@("--theme=$Theme") + $Scene) -PassThru
try {
    Start-Sleep -Milliseconds $SettleMs
    $rect = [SceneShot]::Largest([uint32]$process.Id, $MinWidth)
    if ($rect) {
        [SceneShot]::Capture($rect, $Out)
        Write-Output ("saved {0} ({1}x{2})" -f $Out, $rect[2], $rect[3])
    } else {
        Write-Output 'No window of the scene showed up. Is the scene flag right, and is its plugin in -Plugins?'
        $code = 2
    }
} finally {
    & $Exe --quit | Out-Null
    if ($front -ne [IntPtr]::Zero) { [SceneShot]::SetForegroundWindow($front) | Out-Null }
}
exit $code
