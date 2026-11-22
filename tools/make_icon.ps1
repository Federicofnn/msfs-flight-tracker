# Draws the MSFS Flight Tracker icon and writes assets/icon.ico sizes.
param([string]$outDir = "assets/icon_png")
Add-Type -AssemblyName System.Drawing

$sizes = 16, 24, 32, 48, 64, 128, 256
$S = 1024
$bmp = New-Object System.Drawing.Bitmap $S, $S
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.TextRenderingHint = 'AntiAliasGridFit'
$g.Clear([System.Drawing.Color]::Transparent)

function RoundedRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
  $p = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d = $r * 2
  $p.AddArc($x, $y, $d, $d, 180, 90)
  $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
  $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
  $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
  $p.CloseFigure()
  return $p
}

$bg = RoundedRect 20 20 ($S - 40) ($S - 40) 210
$bgRect = New-Object System.Drawing.Rectangle 0, 0, $S, $S
$bgBrush = [System.Drawing.Drawing2D.LinearGradientBrush]::new($bgRect,
  [System.Drawing.Color]::FromArgb(255, 30, 16, 18),
  [System.Drawing.Color]::FromArgb(255, 8, 7, 9), 90)
$g.FillPath($bgBrush, $bg)
$border = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 52, 30, 33)), 6
$g.DrawPath($border, $bg)

# Orbit ring, tilted.
$state = $g.Save()
$g.TranslateTransform(512, 512)
$g.RotateTransform(-28)
$ringPen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 226, 64, 76)), 22
$g.DrawEllipse($ringPen, -330, -190, 660, 380)
$g.Restore($state)

# Satellite dot on the ring.
$g.FillEllipse((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 236, 72, 84))), 690, 196, 58, 58)
$g.DrawEllipse((New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 120, 28, 36)), 8), 690, 196, 58, 58)

# The F.
$font = New-Object System.Drawing.Font "Segoe UI Black", 470, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
$brush = [System.Drawing.Drawing2D.LinearGradientBrush]::new($bgRect,
  [System.Drawing.Color]::FromArgb(255, 255, 104, 112),
  [System.Drawing.Color]::FromArgb(255, 208, 36, 50), 90)
$format = New-Object System.Drawing.StringFormat
$format.Alignment = 'Center'
$format.LineAlignment = 'Center'
$g.DrawString("F", $font, $brush, (New-Object System.Drawing.RectangleF 0, 0, $S, $S), $format)

New-Item -ItemType Directory -Force -Path $outDir | Out-Null
foreach ($size in $sizes) {
  $small = New-Object System.Drawing.Bitmap $size, $size
  $sg = [System.Drawing.Graphics]::FromImage($small)
  $sg.InterpolationMode = 'HighQualityBicubic'
  $sg.PixelOffsetMode = 'HighQuality'
  $sg.Clear([System.Drawing.Color]::Transparent)
  $sg.DrawImage($bmp, 0, 0, $size, $size)
  $small.Save((Join-Path $outDir "icon_$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $small.Dispose()
}
$bmp.Save((Join-Path $outDir "icon_1024.png"), [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "icons written to $outDir"
