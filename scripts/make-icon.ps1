# SUPERSEDED: the product icon now comes from assets/brand/*.svg via scripts/make-brand-assets.cjs. This script draws the old black-pill icon.
# Generates the 1024x1024 CompanyIsland source icon (black pill, white date dot and line).
#
# Usage:  powershell -File scripts/make-icon.ps1 [-Out app-icon.png]
# Then:   npx tauri icon app-icon.png      (regenerates src-tauri/icons/*)
#
# The generated PNG is a build input, not a repository asset; it is ignored by git.
param([string]$Out = (Join-Path $PSScriptRoot '..\app-icon.png'))

Add-Type -AssemblyName System.Drawing

function New-RoundedRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $d = $r * 2
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $path
}

$size = 1024
$bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.Clear([System.Drawing.Color]::Transparent)

$black = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 10, 10, 10))
$white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 245, 245, 245))

# Pill: 960 x 480, fully rounded, vertically centred.
$pillH = 480
$pillY = ($size - $pillH) / 2
$pill = New-RoundedRect 32 $pillY 960 $pillH ($pillH / 2)
$g.FillPath($black, $pill)
# Thin grey rim keeps the pill visible on dark taskbars and tray backgrounds.
$rim = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 150, 150, 150)), 28
$g.DrawPath($rim, $pill)

# Date dot (left) and weekday line (right).
$dot = 120
$g.FillEllipse($white, 190, ($size - $dot) / 2, $dot, $dot)
$line = New-RoundedRect 400 (($size - 120) / 2) 430 120 60
$g.FillPath($white, $line)

$g.Dispose()
$full = [System.IO.Path]::GetFullPath($Out)
$bmp.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "Wrote $full"
