<#
.SYNOPSIS
Takes the raw pictures for the README, in the light and the dark mode, with
tools\scene-shot.ps1 and its -Demo backdrop, into target\shots\raw\.

.DESCRIPTION
Run tools\compose-readme-shots.ps1 afterwards: it crops, hides and joins the
raw pictures and writes the finished ones to docs\screenshots\.

Build first (cargo build). The pictures show briefly on screen. The palette
queries are chosen so that none of the rows lists the person's own windows,
files or folders.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\take-readme-shots.ps1
#>
[CmdletBinding()]
param([string[]]$Only = @())

$ErrorActionPreference = 'Stop'
$Only = @($Only | ForEach-Object { $_ -split ',' })
$repo = Split-Path -Parent $PSScriptRoot
$raw = Join-Path $repo 'target\shots\raw'
New-Item -ItemType Directory -Force $raw | Out-Null

function Shot([string]$name, [string]$mode, [string]$theme, [string]$plugins, [string]$scene) {
    if ($Only.Count -gt 0 -and $Only -notcontains $name) { return }
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'scene-shot.ps1'),
        '-Scene', $scene, '-Theme', $theme, '-Mode', $mode, '-Demo', '-Out', (Join-Path $raw "$name-$mode.png"))
    if ($plugins) { $arguments += @('-Plugins', $plugins) }
    & powershell.exe @arguments | Select-Object -Last 1
}

foreach ($mode in 'dark', 'light') {
    Shot 'quickactions' $mode 'classic' 'quick_actions' '--open-palette=toggle'
    Shot 'palette-calc' $mode 'classic' '' '--open-palette==2+2*3'
    Shot 'palette-files' $mode 'classic' '' '--open-palette=/C:\Windows\'
    Shot 'palette-terminal' $mode 'classic' '' '--open-palette=>echo hello'
    Shot 'windowdeck' $mode 'classic' 'window_deck' '--open-arrange=demo'
    Shot 'shortcut-detector' $mode 'classic' 'shortcut_detector' '--open-hotkey-dialog=wincraft.palette:Win+L'
    Shot 'store' $mode 'classic' '' '--open-settings=store'
    Shot 'update-ready' $mode 'classic' '' '--show-update-available,--select-update,--open-palette=Update WinCraft'
    Shot 'update-progress' $mode 'classic' '' '--show-update-progress'
    foreach ($theme in 'classic', 'fluent', 'terminal', 'paper', 'high-contrast') {
        Shot "themes-$theme" $mode $theme '' '--open-palette==2+2*3'
    }
}
