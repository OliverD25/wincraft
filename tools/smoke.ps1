<#
.SYNOPSIS
Starts WinCraft once per plugin, then once with every plugin, and checks that
each start is healthy.

.DESCRIPTION
Every run is a test instance (WINCRAFT_INSTANCE=smoke-<plugin>) with its own
scratch LOCALAPPDATA under %TEMP%\wincraft-smoke, so it cannot reach the
WinCraft you are running. The plugin under test is the only one enabled; the
others get a plugins\<id>.json with {"enabled": false}. A test instance is
read-only towards real windows, and ScreenDimmer covers or turns off a monitor
only on its hotkeys or from a saved record, neither of which a fresh scratch
profile has, so no run darkens a screen.

A run passes when the log says "is running" within -StartSeconds, the instance
is still alive after -SettleSeconds more, `--quit` exits with 0, the process
ends with exit code 0, and the log has no ERROR line and no panic.

It shows a tray icon for a few seconds per run, so it is not part of the
pre-push hook; tools\check.ps1 -Smoke runs it.

Exit code: 0 when every run passed, 1 otherwise.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\smoke.ps1
#>
[CmdletBinding()]
param(
    # The build to start. Default: the release build in target\verify, made
    # first when it is missing.
    [string]$Exe,
    [int]$StartSeconds = 20,
    [int]$SettleSeconds = 5
)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not $Exe) {
    $Exe = Join-Path $root 'target\verify\release\wincraft.exe'
    if (-not (Test-Path $Exe)) {
        Write-Host 'building the release exe in target\verify'
        $env:CARGO_TARGET_DIR = 'target\verify'
        cargo build --release --locked
        Remove-Item Env:\CARGO_TARGET_DIR
        if ($LASTEXITCODE -ne 0) { Write-Host 'the build failed'; exit 1 }
    }
}
if (-not (Test-Path $Exe)) { Write-Host "no exe at $Exe"; exit 1 }

$ids = @((Get-Content -Raw (Join-Path $root 'plugins.json') | ConvertFrom-Json).plugins | ForEach-Object { $_.id })
if ($ids.Count -eq 0) { Write-Host 'plugins.json lists no plugins'; exit 1 }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$base = Join-Path ([IO.Path]::GetTempPath()) "wincraft-smoke\$stamp"

function Test-Run {
    param([string]$Label, [string[]]$Enabled)
    $problems = New-Object System.Collections.Generic.List[string]
    $local = Join-Path $base $Label
    $plugins = Join-Path $local 'WinCraft\plugins'
    New-Item -ItemType Directory -Force -Path $plugins | Out-Null
    foreach ($id in $ids) {
        if ($Enabled -notcontains $id) {
            Set-Content -Path (Join-Path $plugins "$id.json") -Value '{"enabled": false}' -Encoding ASCII
        }
    }
    $log = Join-Path $local 'WinCraft\wincraft.log'

    $savedLocal = $env:LOCALAPPDATA
    $savedInstance = $env:WINCRAFT_INSTANCE
    $env:LOCALAPPDATA = $local
    $env:WINCRAFT_INSTANCE = "smoke-$Label"
    $seconds = 0
    try {
        $process = Start-Process -FilePath $Exe -PassThru -WindowStyle Hidden
        $timer = [Diagnostics.Stopwatch]::StartNew()
        $up = $false
        while ($timer.Elapsed.TotalSeconds -lt $StartSeconds) {
            if ($process.HasExited) { break }
            if ((Test-Path $log) -and (Select-String -Path $log -Pattern 'is running' -Quiet)) { $up = $true; break }
            Start-Sleep -Milliseconds 200
        }
        if (-not $up) { $problems.Add("no 'is running' in the log within ${StartSeconds}s") }
        if ($up) {
            Start-Sleep -Seconds $SettleSeconds
            if ($process.HasExited) { $problems.Add("the process ended by itself (exit $($process.ExitCode))") }
        }
        if (-not $process.HasExited) {
            & $Exe --quit | Out-Null
            if ($LASTEXITCODE -ne 0) { $problems.Add("--quit exited with $LASTEXITCODE") }
            if (-not $process.WaitForExit(15000)) {
                $problems.Add('still running 15s after --quit')
                Stop-Process -Id $process.Id -Force
            } elseif ($process.ExitCode -ne 0) {
                $problems.Add("exit code $($process.ExitCode)")
            }
        }
        $seconds = [math]::Round($timer.Elapsed.TotalSeconds, 1)
    } finally {
        $env:LOCALAPPDATA = $savedLocal
        $env:WINCRAFT_INSTANCE = $savedInstance
    }
    if (Test-Path $log) {
        $bad = @(Select-String -Path $log -Pattern '^\S+ \S+ ERROR |panic' | ForEach-Object { $_.Line })
        foreach ($line in $bad | Select-Object -First 3) { $problems.Add("log: $line") }
    } else {
        $problems.Add('no log file was written')
    }
    [pscustomobject]@{
        Run     = $Label
        Seconds = $seconds
        Result  = if ($problems.Count -eq 0) { 'ok' } else { 'FAIL' }
        Detail  = ($problems -join '; ')
    }
}

$rows = @()
foreach ($id in $ids) { $rows += Test-Run $id @($id) }
$rows += Test-Run 'all' $ids

$rows | Format-Table -AutoSize -Wrap | Out-String -Width 200 | Write-Host
Write-Host "logs and profiles: $base"
if ($rows | Where-Object { $_.Result -ne 'ok' }) { exit 1 }
exit 0
