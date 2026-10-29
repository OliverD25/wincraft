<#
.SYNOPSIS
Turns the raw pictures of tools\take-readme-shots.ps1 into the finished ones
in docs\screenshots\: crops the edges, hides what must not show, and joins
several pictures into one.

.DESCRIPTION
Reads target\shots\raw\<name>-<mode>.png and writes docs\screenshots\. Run it
again after every new take; it always starts from the raw pictures.

- Every picture loses 3 px at each edge (the border of the capture).
- shortcut-detector is cropped to the hotkey dialog.
- Only the pictures the README uses are written. In updates, the rows under "Update WinCraft" are covered, because a
  search for it also finds the person's own files.
- palette-tricks-<mode>: the three palette searches, one above the other.
- themes-<mode>: the five themes with their names under them.
- updates-<mode>: the "Update ready" row above the progress panel.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools\compose-readme-shots.ps1
#>
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$repo = Split-Path -Parent $PSScriptRoot
$raw = Join-Path $repo 'target\shots\raw'
$out = Join-Path $repo 'docs\screenshots'
New-Item -ItemType Directory -Force $out | Out-Null

function Load([string]$name, [string]$mode) {
    $path = Join-Path $raw "$name-$mode.png"
    $loaded = [System.Drawing.Bitmap]::FromFile($path)
    $copy = New-Object System.Drawing.Bitmap($loaded)
    $loaded.Dispose()
    $copy
}

function Crop([System.Drawing.Bitmap]$bmp, [int]$x, [int]$y, [int]$w, [int]$h) {
    $rect = New-Object System.Drawing.Rectangle($x, $y, $w, $h)
    $bmp.Clone($rect, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
}

function Trim-Edge([System.Drawing.Bitmap]$bmp) { Crop $bmp 3 3 ($bmp.Width - 6) ($bmp.Height - 6) }

# The palette panel without its shadow margin, from the top down to $height.
# Themes with glass have no margin, so the margin is worked out from the width.
function Panel-Top([System.Drawing.Bitmap]$bmp, [int]$height) {
    $margin = [int](($bmp.Width - 1347) / 2)
    Crop $bmp $margin $margin ($bmp.Width - 2 * $margin) $height
}

function Save([System.Drawing.Bitmap]$bmp, [string]$name, [int]$maxWidth = 0) {
    if ($maxWidth -gt 0 -and $bmp.Width -gt $maxWidth) {
        $bmp = Scale $bmp ($maxWidth / $bmp.Width)
    }
    $path = Join-Path $out $name
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Output ("{0}  {1}x{2}  {3} KB" -f $name, $bmp.Width, $bmp.Height, [int]((Get-Item $path).Length / 1KB))
}

function Scale([System.Drawing.Bitmap]$bmp, [double]$factor) {
    $w = [int]($bmp.Width * $factor); $h = [int]($bmp.Height * $factor)
    $result = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($result)
    $g.InterpolationMode = 'HighQualityBicubic'; $g.SmoothingMode = 'HighQuality'; $g.PixelOffsetMode = 'HighQuality'
    $g.DrawImage($bmp, 0, 0, $w, $h)
    $g.Dispose()
    $result
}

function Canvas([int]$w, [int]$h, [string]$mode) {
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear($(if ($mode -eq 'dark') { [System.Drawing.Color]::FromArgb(20, 22, 28) } else { [System.Drawing.Color]::FromArgb(226, 228, 233) }))
    $g.Dispose()
    $bmp
}

function Draw([System.Drawing.Bitmap]$canvas, [System.Drawing.Bitmap]$image, [int]$x, [int]$y) {
    $g = [System.Drawing.Graphics]::FromImage($canvas)
    $g.DrawImage($image, $x, $y, $image.Width, $image.Height)
    $g.Dispose()
}

function Label([System.Drawing.Bitmap]$canvas, [string]$text, [int]$centerX, [int]$y, [string]$mode) {
    $g = [System.Drawing.Graphics]::FromImage($canvas)
    $g.TextRenderingHint = 'ClearTypeGridFit'
    $font = New-Object System.Drawing.Font('Segoe UI', 18, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    $brush = New-Object System.Drawing.SolidBrush($(if ($mode -eq 'dark') { [System.Drawing.Color]::FromArgb(235, 235, 240) } else { [System.Drawing.Color]::FromArgb(32, 32, 36) }))
    $size = $g.MeasureString($text, $font)
    $g.DrawString($text, $font, $brush, [single]($centerX - $size.Width / 2), [single]$y)
    $g.Dispose()
}

foreach ($mode in 'dark', 'light') {
    # Single pictures.
    foreach ($name in 'quickactions', 'windowdeck', 'store') {
        Save (Trim-Edge (Load $name $mode)) "$name-$mode.png"
    }
    Save (Crop (Load 'shortcut-detector' $mode) 470 313 620 424) "shortcut-detector-$mode.png"

    $ready = Load 'update-ready' $mode
    $g = [System.Drawing.Graphics]::FromImage($ready)
    $cover = $ready.GetPixel(500, 700)
    $g.FillRectangle((New-Object System.Drawing.SolidBrush($cover)), 40, 245, 894, 511)
    $g.Dispose()

    # Palette tricks: calculator, folders and files, terminal commands.
    # The folder list is cut after a whole row; the other two show all of their inspector.
    $width = 960; $gap = 16
    $heights = @{ 'palette-calc' = 420; 'palette-files' = 388; 'palette-terminal' = 420 }
    $shots = foreach ($name in 'palette-calc', 'palette-files', 'palette-terminal') { $p = Panel-Top (Load $name $mode) $heights[$name]; Scale $p ($width / $p.Width) }
    $total = 40 + $gap * 2 + ($shots | ForEach-Object { $_.Height } | Measure-Object -Sum).Sum
    $canvas = Canvas ($width + 40) $total $mode
    $y = 20
    foreach ($shot in $shots) { Draw $canvas $shot 20 $y; $y += $shot.Height + $gap }
    Save $canvas "palette-tricks-$mode.png"

    # Themes: two columns, the fifth one in the middle of the last row.
    $names = 'classic', 'fluent', 'terminal', 'paper', 'high-contrast'
    $labels = 'Classic', 'Fluent', 'Terminal', 'Paper', 'High contrast'
    $cell = 800; $labelHeight = 34; $gap = 18
    $themeShots = $names | ForEach-Object { $p = Panel-Top (Load "themes-$_" $mode) 432; Scale $p ($cell / $p.Width) }
    $each = $themeShots[0].Height
    $rowHeight = $each + $labelHeight + $gap
    $canvas = Canvas (2 * $cell + $gap + 40) (3 * $rowHeight + 40) $mode
    for ($i = 0; $i -lt 5; $i++) {
        $row = [math]::Floor($i / 2)
        $x = if ($i -eq 4) { 20 + [int](($cell + $gap) / 2) } else { 20 + ($i % 2) * ($cell + $gap) }
        $y = 20 + $row * $rowHeight
        Draw $canvas $themeShots[$i] $x $y
        Label $canvas $labels[$i] ($x + [int]($cell / 2)) ($y + $each + 4) $mode
    }
    Save $canvas "themes-$mode.png"

    # Updates: the ready row above the progress panel.
    $p = Panel-Top $ready 360; $top = Scale $p (900 / $p.Width)
    $panel = Trim-Edge (Load 'update-progress' $mode)
    $canvas = Canvas (900 + 40) ($top.Height + $panel.Height + 16 + 40) $mode
    Draw $canvas $top 20 20
    Draw $canvas $panel (20 + [int]((900 - $panel.Width) / 2)) (20 + $top.Height + 16)
    Save $canvas "updates-$mode.png"
}
