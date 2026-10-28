<#
.SYNOPSIS
Writes docs\test-map.md: which tests belong to which feature, which features
have few tests, which flags nobody uses, and which tests look like suspects.

.DESCRIPTION
Plain text parsing only. It does not run cargo, does not change any source
file, and does not touch %LOCALAPPDATA% or any window or setting. The only
file it writes is docs\test-map.md. The output is sorted, so a second run with
no code change gives the same file apart from the date line.

.PARAMETER Out
Where to write the report. Default: docs\test-map.md.

.EXAMPLE
pwsh -NoProfile -File tools\test-map.ps1
#>
[CmdletBinding()]
param(
    [string]$Out = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if (-not $Out) { $Out = Join-Path $root 'docs\test-map.md' }

# A feature with fewer tests than this is listed as thin.
$FewTests = 3

function Get-Lines([string]$path) { , [IO.File]::ReadAllLines((Join-Path $root $path)) }
function Rel([string]$full) { ($full.Substring($root.Length + 1)) -replace '\\', '/' }
# Ordinal sort, so the order is the same on every machine.
function Sorted($items) {
    $list = [string[]]@($items)
    [Array]::Sort($list, [StringComparer]::Ordinal)
    , $list
}

$sourceFiles = Sorted (Get-ChildItem -Path (Join-Path $root 'src') -Recurse -Filter '*.rs' -File |
        ForEach-Object { Rel $_.FullName })

# ------------------------------------------------------------ feature of a path
function Get-Feature([string]$path) {
    if ($path -eq 'src/main.rs') { return 'main: command line' }
    if ($path -match '^src/plugins/([^/]+)/') { return "plugin: $($Matches[1])" }
    if ($path -match '^src/plugins/[^/]+\.rs$') { return 'plugins: contract' }
    if ($path -match '^src/(core|ui)/([^/.]+)') { return "$($Matches[1]): $($Matches[2])" }
    if ($path -match '^src/(search|store)/') { return $Matches[1] }
    return 'other'
}

# ------------------------------------------------------------ tests per file
$fileTests = @{}
foreach ($file in $sourceFiles) {
    $tests = 0; $ignored = 0
    foreach ($line in (Get-Lines $file)) {
        if ($line -match '^\s*#\[(tokio::)?test\b') { $tests++ }
        elseif ($line -match '^\s*#\[ignore\b') { $ignored++ }
    }
    if ($tests -gt 0) { $fileTests[$file] = @{ Tests = $tests; Ignored = $ignored } }
}

$featureTests = @{}
foreach ($file in $fileTests.Keys) {
    $feature = Get-Feature $file
    if (-not $featureTests.ContainsKey($feature)) {
        $featureTests[$feature] = @{ Tests = 0; Ignored = 0; Files = @() }
    }
    $featureTests[$feature].Tests += $fileTests[$file].Tests
    $featureTests[$feature].Ignored += $fileTests[$file].Ignored
    $featureTests[$feature].Files += $file
}
$totalTests = ($fileTests.Values | ForEach-Object { $_.Tests } | Measure-Object -Sum).Sum
$totalIgnored = ($fileTests.Values | ForEach-Object { $_.Ignored } | Measure-Object -Sum).Sum

# ------------------------------------------------------------ inventory
$allFeatures = @{}
foreach ($file in $sourceFiles) { $allFeatures[(Get-Feature $file)] = $true }

$pluginsJson = Get-Content -Raw (Join-Path $root 'plugins.json') | ConvertFrom-Json
$pluginIds = Sorted ($pluginsJson.plugins | ForEach-Object { $_.id })
$pluginDirs = Sorted (Get-ChildItem (Join-Path $root 'src/plugins') -Directory | ForEach-Object { $_.Name })

function Get-Captures([string]$text, [string]$pattern) {
    @([regex]::Matches($text, $pattern, 'Singleline') | ForEach-Object { $_.Groups[1].Value })
}

# The code of a file above its first #[cfg(test)] line. Test code must not
# count as a feature or as a user of a flag.
function Get-NonTestText([string]$path) {
    $keep = New-Object System.Collections.Generic.List[string]
    foreach ($line in (Get-Lines $path)) {
        if ($line -match '^\s*#\[cfg\(test\)\]') { break }
        $keep.Add($line)
    }
    $keep -join "`n"
}

# Per plugin: hotkeys, settings fields and palette commands, read from the
# struct literals in its source.
$pluginInventory = @()
foreach ($id in $pluginDirs) {
    $text = ''
    foreach ($file in $sourceFiles | Where-Object { $_ -like "src/plugins/$id/*" -and $_ -notlike '*/tests.rs' }) {
        $text += (Get-NonTestText $file) + "`n"
    }
    $pluginInventory += [pscustomobject]@{
        Id = $id; InJson = ($pluginIds -contains $id)
        Hotkeys = (Sorted (Get-Captures $text 'HotkeyAction\s*\{\s*id:[^,]+,\s*name:[^,]+,\s*label:\s*"([^"]*)"'))
        Fields = (Sorted (Get-Captures $text 'SettingField\s*\{\s*key:\s*"([^"]*)"'))
        Commands = (Sorted (Get-Captures $text 'PaletteCommand\s*\{\s*id:[^,]+,\s*label:\s*"([^"]*)"'))
    }
}

# Command-line flags: every "--name" string in the non-test code of src/,
# except the search providers (their strings are shell text a user types).
$flagDefs = @{}
foreach ($file in $sourceFiles) {
    if ($file -like 'src/search/*' -or $file -like '*/tests.rs') { continue }
    foreach ($m in [regex]::Matches((Get-NonTestText $file), '"(--[a-z][a-z-]*)')) {
        $flag = $m.Groups[1].Value
        if (-not $flagDefs.ContainsKey($flag)) { $flagDefs[$flag] = @() }
        if ($flagDefs[$flag] -notcontains $file) { $flagDefs[$flag] += $file }
    }
}

# Where a flag can be used from. The code files that parse a flag are not in
# this list: they are the handler, not a user.
$userFiles = @()
$userFiles += Get-ChildItem (Join-Path $root 'tools') -File |
    Where-Object { $_.Extension -in '.ps1', '.sh' -and $_.Name -ne 'test-map.ps1' } | ForEach-Object { Rel $_.FullName }
$userFiles += @('README.md', 'CONTRIBUTING.md', 'CLAUDE.md', 'build.rs') | Where-Object { Test-Path (Join-Path $root $_) }
$userFiles += Get-ChildItem (Join-Path $root 'docs') -Recurse -File -Filter '*.md' |
    Where-Object { $_.Name -ne 'test-map.md' } | ForEach-Object { Rel $_.FullName }
$userFiles += Get-ChildItem (Join-Path $root 'src') -Recurse -File -Filter '*.md' | ForEach-Object { Rel $_.FullName }
if (Test-Path (Join-Path $root '.github')) {
    $userFiles += Get-ChildItem (Join-Path $root '.github') -Recurse -File | ForEach-Object { Rel $_.FullName }
}
$userFiles = Sorted ($userFiles | Select-Object -Unique)
$userText = @{}
foreach ($file in $userFiles) { $userText[$file] = [IO.File]::ReadAllText((Join-Path $root $file)) }

# Code that starts WinCraft again with a flag, or builds a command line for it.
$selfStarters = @('src/core/install.rs', 'src/core/update.rs', 'src/core/autostart.rs')

$flagRows = @()
foreach ($flag in (Sorted $flagDefs.Keys)) {
    $pattern = [regex]::Escape($flag) + '(?![a-z-])'
    $users = @($userFiles | Where-Object { $userText[$_] -match $pattern })
    $defs = Sorted $flagDefs[$flag]
    $selfUse = @($defs | Where-Object { $selfStarters -contains $_ })
    $flagRows += [pscustomobject]@{
        Flag = $flag; Defined = $defs; Users = $users; SelfUse = $selfUse
        Nobody = ($users.Count -eq 0 -and $selfUse.Count -eq 0)
    }
}

$shots = [IO.File]::ReadAllText((Join-Path $root 'tools/screenshots.ps1'))
$sceneNames = Sorted ([regex]::Matches($shots, "@\{\s*Name\s*=\s*'([^']+)'") | ForEach-Object { $_.Groups[1].Value } | Select-Object -Unique)

$storyFiles = @()
if (Test-Path (Join-Path $root 'docs/user-stories')) {
    $storyFiles = Sorted (Get-ChildItem (Join-Path $root 'docs/user-stories') -File | ForEach-Object { $_.Name })
}

# ------------------------------------------------------------ suspects
$suspects = @()
foreach ($row in $flagRows) {
    if ($row.Nobody) {
        $suspects += "Flag ``$($row.Flag)``: no tool, document or installer uses it (defined in $($row.Defined -join ', '))."
    }
}
foreach ($row in $flagRows) {
    if (-not $row.Nobody -and $row.SelfUse.Count -eq 0 -and -not ($row.Users | Where-Object { $_ -like 'tools/*' })) {
        $suspects += "Flag ``$($row.Flag)``: only documents mention it; no script or installer uses it ($($row.Users -join ', '))."
    }
}
foreach ($dir in $pluginDirs) {
    if ($pluginIds -notcontains $dir) { $suspects += "Plugin folder ``src/plugins/$dir``: not listed in plugins.json." }
}
foreach ($id in $pluginIds) {
    if ($pluginDirs -notcontains $id) { $suspects += "plugins.json lists ``$id`` but there is no folder src/plugins/$id." }
}
foreach ($file in (Sorted $fileTests.Keys)) {
    if ($file -match '(thumbnail_probe|hide_test)') {
        $suspects += "Test module ``$file`` ($($fileTests[$file].Tests) tests): in a one-off probe (thumbnail_probe)."
    }
}

# ------------------------------------------------------------ write
$doc = New-Object System.Collections.Generic.List[string]
function Add-Line([string]$line = '') { $script:doc.Add($line) }

Add-Line '# WinCraft test map'
Add-Line ''
Add-Line "Generated by ``tools/test-map.ps1`` on $(Get-Date -Format 'yyyy-MM-dd'). Do not edit by hand."
Add-Line 'Run `pwsh -NoProfile -File tools/test-map.ps1` to refresh it. The script only reads files.'
Add-Line ''
Add-Line "Total: **$totalTests** tests in $($fileTests.Count) files ($totalIgnored marked ``#[ignore]``)."
Add-Line ''
Add-Line 'A "feature" comes from the path of the file: `src/plugins/<name>/` is that plugin,'
Add-Line '`src/core/<x>` is `core: <x>`, `src/ui/<x>` is `ui: <x>`, and so on. Tests are counted'
Add-Line 'by `#[test]` lines, in `tests.rs` files and in `#[cfg(test)]` blocks alike.'
Add-Line ''
Add-Line '## (a) Tests per feature'
Add-Line ''
Add-Line '| Feature | Tests | Ignored | Files |'
Add-Line '|---|---:|---:|---:|'
foreach ($feature in (Sorted $featureTests.Keys)) {
    $f = $featureTests[$feature]
    Add-Line "| $feature | $($f.Tests) | $($f.Ignored) | $($f.Files.Count) |"
}
Add-Line ''
Add-Line '### Test modules by feature'
Add-Line ''
foreach ($feature in (Sorted $featureTests.Keys)) {
    Add-Line "**$feature**"
    Add-Line ''
    foreach ($file in (Sorted $featureTests[$feature].Files)) {
        $n = $fileTests[$file]
        $extra = if ($n.Ignored -gt 0) { ", $($n.Ignored) ignored" } else { '' }
        Add-Line "- ``$file``: $($n.Tests)$extra"
    }
    Add-Line ''
}

Add-Line '## (b) Features with no tests or few tests'
Add-Line ''
Add-Line "Few means fewer than $FewTests tests. Every folder or file group with source code is a"
Add-Line 'feature here. A thin count is a question, not always a gap: a small module may be'
Add-Line 'tested through its caller.'
Add-Line ''
Add-Line '| Feature | Tests |'
Add-Line '|---|---:|'
$thin = 0
foreach ($feature in (Sorted $allFeatures.Keys)) {
    $n = if ($featureTests.ContainsKey($feature)) { $featureTests[$feature].Tests } else { 0 }
    if ($n -lt $FewTests) { Add-Line "| $feature | $n |"; $thin++ }
}
if ($thin -eq 0) { Add-Line '| (none) | |' }
Add-Line ''
Add-Line '### Plugins in the inventory'
Add-Line ''
Add-Line '| Plugin | In plugins.json | Tests | Hotkeys | Settings fields | Palette commands |'
Add-Line '|---|---|---:|---:|---:|---:|'
foreach ($p in $pluginInventory) {
    $key = "plugin: $($p.Id)"
    $n = if ($featureTests.ContainsKey($key)) { $featureTests[$key].Tests } else { 0 }
    $json = if ($p.InJson) { 'yes' } else { 'NO' }
    Add-Line "| $($p.Id) | $json | $n | $($p.Hotkeys.Count) | $($p.Fields.Count) | $($p.Commands.Count) |"
}
Add-Line ''
Add-Line 'Counts of hotkeys, fields and commands come from the struct literals in the plugin code.'
Add-Line 'A plugin that builds them in a loop or from a table shows fewer than it has.'
Add-Line ''
foreach ($p in $pluginInventory) {
    if ($p.Hotkeys.Count -eq 0 -and $p.Fields.Count -eq 0 -and $p.Commands.Count -eq 0) { continue }
    Add-Line "**$($p.Id)**"
    Add-Line ''
    if ($p.Hotkeys.Count) { Add-Line ('- hotkeys: ' + (($p.Hotkeys | ForEach-Object { "``$_``" }) -join ', ')) }
    if ($p.Fields.Count) { Add-Line ('- settings fields: ' + (($p.Fields | ForEach-Object { "``$_``" }) -join ', ')) }
    if ($p.Commands.Count) { Add-Line ('- palette commands: ' + (($p.Commands | ForEach-Object { "``$_``" }) -join ', ')) }
    Add-Line ''
}
Add-Line '### Other inventory lists'
Add-Line ''
Add-Line "- Screenshot scenes in tools/screenshots.ps1 ($($sceneNames.Count), plus one per -Query):"
Add-Line ('  ' + (($sceneNames | ForEach-Object { "``$_``" }) -join ', '))
if ($storyFiles.Count) {
    Add-Line "- User stories in docs/user-stories ($($storyFiles.Count)): " + (($storyFiles | ForEach-Object { "``$_``" }) -join ', ')
} else {
    Add-Line '- User stories: the folder docs/user-stories does not exist yet.'
}
Add-Line ''

Add-Line '## (c) Suspects'
Add-Line ''
Add-Line 'A suspect is a flag that nobody uses, a flag that only documents mention, a test module of a one-off probe,'
Add-Line 'or a plugin that is not in `plugins.json`. Each one is a question for the owner.'
Add-Line 'Nothing here is removed automatically. A test written for a bug that reached a user'
Add-Line 'stays (Principle 7).'
Add-Line ''
if ($suspects.Count) { foreach ($s in $suspects) { Add-Line "- $s" } } else { Add-Line '- (none)' }
Add-Line ''

Add-Line '## (d) Command-line flags: who uses each one'
Add-Line ''
Add-Line '"Used by" lists files outside the parsing code that name the flag: scripts in `tools/`,'
Add-Line 'the docs, the README files, `.github`. "Self-start" means the install, update or'
Add-Line 'autostart code builds the flag itself. "nobody" means only the code that parses it'
Add-Line 'mentions it.'
Add-Line ''
Add-Line '| Flag | Defined in | Used by |'
Add-Line '|---|---|---|'
foreach ($row in $flagRows) {
    $by = @()
    if ($row.Users.Count) { $by += ($row.Users | ForEach-Object { "``$_``" }) }
    if ($row.SelfUse.Count) { $by += "self-start ($($row.SelfUse -join ', '))" }
    if ($row.Nobody) { $by = @('nobody') }
    $defs = ($row.Defined | ForEach-Object { "``$_``" }) -join ', '
    Add-Line "| ``$($row.Flag)`` | $defs | $($by -join ', ') |"
}
Add-Line ''

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null
[IO.File]::WriteAllText($Out, (($doc -join "`n") + "`n"), (New-Object Text.UTF8Encoding($false)))
Write-Host ("wrote {0}: {1} tests, {2} flags, {3} suspects" -f (Rel $Out), $totalTests, $flagRows.Count, $suspects.Count)
