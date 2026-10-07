# Draws the NSIS installer images from the brand (ADR 0012): the side image
# of the welcome and finish pages (164x314) and the header of the other pages
# (150x57), both 24-bit BMP as NSIS wants. Run from the repository root:
#   powershell -ExecutionPolicy Bypass -File assets/installer/make-images.ps1
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$icon = [System.Drawing.Image]::FromFile((Join-Path $root 'assets/brand/app-icon-512.png'))
$fonts = New-Object System.Drawing.Text.PrivateFontCollection
$fonts.AddFontFile((Join-Path $root 'assets/fonts/BricolageGrotesque-Variable.ttf'))
$family = $fonts.Families[0]

# Brand colours (crates/cloudrs-ui/src/tokens.rs, dark theme).
$deep = [System.Drawing.Color]::FromArgb(0x0D, 0x0C, 0x0B)
$canvas = [System.Drawing.Color]::FromArgb(0x1B, 0x19, 0x16)
$accent = [System.Drawing.Color]::FromArgb(0xFF, 0x55, 0x00)
$text = [System.Drawing.Color]::FromArgb(0xF5, 0xF1, 0xEC)
$muted = [System.Drawing.Color]::FromArgb(0xA8, 0x9F, 0x95)

function New-Canvas([int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    return $bmp, $g
}

# The wordmark: "cloud" in the text colour, "rs" in the accent.
function Draw-Wordmark($g, [float]$size, [float]$cx, [float]$y) {
    $font = New-Object System.Drawing.Font $family, $size, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
    $format = [System.Drawing.StringFormat]::GenericTypographic
    $a = $g.MeasureString('cloud', $font, 1000, $format).Width
    $b = $g.MeasureString('rs', $font, 1000, $format).Width
    $x = $cx - ($a + $b) / 2
    $g.DrawString('cloud', $font, (New-Object System.Drawing.SolidBrush $text), $x, $y, $format)
    $g.DrawString('rs', $font, (New-Object System.Drawing.SolidBrush $accent), $x + $a, $y, $format)
}

# Side image: deep warm background, an orange glow behind the icon, the
# wordmark and a short line under it.
$bmp, $g = New-Canvas 164 314
$g.Clear($deep)
$glow = New-Object System.Drawing.Drawing2D.GraphicsPath
$glow.AddEllipse(-60, -20, 284, 230)
$brush = New-Object System.Drawing.Drawing2D.PathGradientBrush $glow
$brush.CenterColor = [System.Drawing.Color]::FromArgb(90, $accent)
$brush.SurroundColors = @([System.Drawing.Color]::FromArgb(0, $deep))
$g.FillPath($brush, $glow)
$g.DrawImage($icon, 34, 64, 96, 96)
Draw-Wordmark $g 30 82 176
$small = New-Object System.Drawing.Font $family, 11, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$center = New-Object System.Drawing.StringFormat
$center.Alignment = 'Center'
$g.DrawString('SoundCloud, native.', $small, (New-Object System.Drawing.SolidBrush $muted), (New-Object System.Drawing.RectangleF 0, 216, 164, 20), $center)
$g.FillRectangle((New-Object System.Drawing.SolidBrush $accent), 70, 244, 24, 3)
$g.Dispose()
$bmp.Save((Join-Path $PSScriptRoot 'sidebar.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()

# Header: the page's own background on the left fading into the brand, the
# icon on the right where NSIS leaves room for it.
$bmp, $g = New-Canvas 150 57
$fade = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point 150, 0), $canvas, $deep
$g.FillRectangle($fade, 0, 0, 150, 57)
$g.DrawImage($icon, 99, 8, 41, 41)
$g.Dispose()
$bmp.Save((Join-Path $PSScriptRoot 'header.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()
$icon.Dispose()
'sidebar.bmp and header.bmp written'
