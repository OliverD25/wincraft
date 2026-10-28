# Sums up a WinCraft detailed log (Settings > General > Detailed log), so a
# person or a Claude session reads one page instead of the whole file.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\detailed-log.ps1 <file>
#
# With no file it takes the newest one in %LOCALAPPDATA%\WinCraft\detailed.
# It prints plain text, in this order:
#   Session      the ON and OFF lines and the session header
#   Marks        the moments the user marked, with their words
#   Win+digit    one block per press of a Windows key: the hook lines next to
#                the key-state watcher, times from the start of the press,
#                gaps over 100 ms marked, and every digit Windows saw that no
#                WinCraft hook line names marked "!! NO HOOK"
#   Busy         host busy, slow UI frames and slow plugin calls
#   Front        every change of the window in front
#   Counts       lines per kind
# It only reads the file.
param([string]$Path = '')

$ErrorActionPreference = 'Stop'

if (-not $Path) {
    $folder = Join-Path $env:LOCALAPPDATA 'WinCraft\detailed'
    $newest = Get-ChildItem -Path $folder -Filter 'detailed-*.log' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $newest) {
        Write-Output "No detailed log in $folder"
        exit 1
    }
    $Path = $newest.FullName
}
if (-not (Test-Path -LiteralPath $Path)) {
    Write-Output "No such file: $Path"
    exit 1
}

$pattern = '^(\d{4}-\d\d-\d\d) (\d\d):(\d\d):(\d\d)\.(\d{3}) \[([^\]]*)\] (.*)$'
$entries = New-Object System.Collections.Generic.List[object]
# WinCraft keeps the newest file open while it records, so it is read with
# the writer's share mode; ReadAllLines would refuse a file open for writing.
$stream = [System.IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
$reader = New-Object System.IO.StreamReader($stream)
$lines = New-Object System.Collections.Generic.List[string]
while ($null -ne ($line = $reader.ReadLine())) { $lines.Add($line) }
$reader.Close()
foreach ($raw in $lines) {
    if ($raw -notmatch $pattern) { continue }
    $ms = ((([int]$Matches[2] * 60) + [int]$Matches[3]) * 60 + [int]$Matches[4]) * 1000 + [int]$Matches[5]
    $time = "$($Matches[2]):$($Matches[3]):$($Matches[4]).$($Matches[5])"
    $thread = $Matches[6]
    $text = $Matches[7]
    $kind = if ($text.StartsWith('=== ')) { ($text -split ' ')[1] }
            elseif ($text -match '^host busy') { 'busy' }
            elseif ($text -match '^ui frame slow') { 'busy' }
            else { ($text -split '[ :]')[0] }
    $entries.Add([pscustomobject]@{
        Ms = $ms
        Time = $time
        Thread = $thread
        Text = $text
        Kind = $kind
    })
}

function Show([string]$title) {
    Write-Output ''
    Write-Output "== $title"
}

Write-Output "Detailed log: $Path"
Write-Output "$($entries.Count) lines"

Show 'Session'
foreach ($e in $entries) {
    if ($e.Kind -in 'ON', 'OFF', 'header') { Write-Output "$($e.Time)  $($e.Text)" }
}

Show 'Marks'
$marks = @($entries | Where-Object { $_.Kind -eq 'MARK' })
if ($marks.Count -eq 0) { Write-Output '(none)' }
foreach ($e in $marks) { Write-Output "$($e.Time)  $($e.Text)" }

# A press starts when a Windows key goes down (a hook line or the watcher)
# and ends when the watcher reads no key, or the Windows key's release when
# the watcher never ran.
$winDown = '^key [^:]+: [LR]Win down'
$winUp = '^key [^:]+: [LR]Win up'
$presses = New-Object System.Collections.Generic.List[object]
$current = $null
$sawWatcher = $false
foreach ($e in $entries) {
    $starts = ($e.Text -match $winDown) -or ($e.Text -match '^keystate: .*Win')
    if ($null -eq $current -and $starts) {
        $current = New-Object System.Collections.Generic.List[object]
        $sawWatcher = $false
    }
    if ($null -eq $current) { continue }
    if ($e.Kind -notin 'log', 'header') { $current.Add($e) }
    if ($e.Kind -eq 'keystate') { $sawWatcher = $true }
    $ends = ($e.Text -eq 'keystate: none') -or (($e.Text -match $winUp) -and -not $sawWatcher)
    if ($ends) {
        $presses.Add($current)
        $current = $null
    }
}
if ($null -ne $current) { $presses.Add($current) }

function Digits([string]$text) {
    $found = @()
    if ($text -match '^keystate: (.*)$') {
        foreach ($part in ($Matches[1] -split '\+')) {
            if ($part -match '^\d$') { $found += $part }
        }
    }
    , $found
}

Show 'Win+digit'
if ($presses.Count -eq 0) { Write-Output '(no Windows key pressed)' }
$number = 0
$missing = 0
foreach ($press in $presses) {
    $number++
    $start = $press[0].Ms
    Write-Output ''
    Write-Output "Press $number at $($press[0].Time)"
    $before = @()
    $previous = $start
    foreach ($e in $press) {
        $gap = $e.Ms - $previous
        if ($gap -gt 100) { Write-Output ("          -- gap {0} ms --" -f $gap) }
        $previous = $e.Ms
        Write-Output ("  +{0,5} ms  {1}" -f ($e.Ms - $start), $e.Text)
        if ($e.Kind -eq 'keystate') {
            $now = Digits $e.Text
            foreach ($digit in $now) {
                if ($before -contains $digit) { continue }
                $hooked = @($press | Where-Object {
                    $_.Kind -eq 'key' -and $_.Text -match "^key [^:]+: $digit down" -and
                    [math]::Abs($_.Ms - $e.Ms) -le 100
                })
                if ($hooked.Count -eq 0) {
                    $missing++
                    Write-Output "          !! NO HOOK: Windows saw digit $digit down, but no WinCraft hook line names it"
                }
            }
            $before = $now
        }
    }
}
if ($missing -gt 0) {
    Write-Output ''
    Write-Output "$missing digit press(es) reached Windows without a WinCraft hook line."
}

Show 'Busy'
$busy = @($entries | Where-Object { $_.Kind -eq 'busy' -or $_.Text -match ' SLOW$' })
if ($busy.Count -eq 0) { Write-Output '(nothing slow)' }
foreach ($e in $busy) { Write-Output "$($e.Time)  $($e.Text)" }

Show 'Front'
$front = @($entries | Where-Object { $_.Kind -in 'front', 'front-ask' })
if ($front.Count -eq 0) { Write-Output '(no change)' }
foreach ($e in $front) { Write-Output "$($e.Time)  $($e.Text)" }

Show 'Counts'
$entries | Group-Object Kind | Sort-Object Count -Descending | ForEach-Object {
    Write-Output ("{0,6}  {1}" -f $_.Count, $_.Name)
}
