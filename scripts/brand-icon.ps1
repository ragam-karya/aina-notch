# aina's brand images, from the logo files in crates/badi-app/icons/brand (drawn on a plain background):
#   icons/source.png          the mark alone, background keyed out, centered on a transparent 1024 px
#                             square; `npm run icon` runs this, then `tauri icon` makes every size Tauri
#                             and the MSIX need from it
#   brand/aina-mark.png       the mark alone (no name), a copy of icons/source.png, for places too
#                             small or too square for the name: avatars, app tiles, the notch
#   brand/aina-logo.png       the logo with the name in dark text, for light backgrounds, on transparent
#   brand/aina-logo-dark.png  the same with white text, for dark backgrounds
#   installer/sidebar.bmp, header.bmp  the NSIS installer's images
param([double]$Margin = 0.08)
$ErrorActionPreference = 'Stop'
$icons = Join-Path $PSScriptRoot '..\crates\badi-app\icons'
$brand = Join-Path $icons 'brand'
$installer = Join-Path $PSScriptRoot '..\crates\badi-app\installer'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class BrandIcon {
  // The background color `bg` (and the paper noise around it) becomes transparent; the edges keep a
  // soft alpha with the background taken out of their color.
  public static Bitmap KeyOut(Bitmap src, Color bg, int lo, int hi) {
    var bmp = new Bitmap(src.Width, src.Height, PixelFormat.Format32bppArgb);
    using (var g = Graphics.FromImage(bmp)) g.DrawImage(src, 0, 0, src.Width, src.Height);
    var data = bmp.LockBits(new Rectangle(0, 0, bmp.Width, bmp.Height), ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
    var px = new byte[data.Stride * bmp.Height];
    Marshal.Copy(data.Scan0, px, 0, px.Length);
    int[] k = { bg.B, bg.G, bg.R };
    for (int i = 0; i < px.Length; i += 4) {
      int d = 0;
      for (int c = 0; c < 3; c++) d = Math.Max(d, Math.Abs(px[i + c] - k[c]));
      double a = Math.Min(1.0, Math.Max(0.0, (d - lo) / (double)(hi - lo)));
      if (a <= 0) { px[i] = px[i + 1] = px[i + 2] = px[i + 3] = 0; continue; }
      for (int c = 0; c < 3; c++) px[i + c] = (byte)Math.Min(255.0, Math.Max(0.0, (px[i + c] - (1 - a) * k[c]) / a));
      px[i + 3] = (byte)Math.Round(a * 255);
    }
    Marshal.Copy(px, 0, data.Scan0, px.Length);
    bmp.UnlockBits(data);
    return bmp;
  }

  // The smallest rectangle holding pixels with alpha above 10%, from column `fromX` on.
  public static Rectangle Bounds(Bitmap bmp, int fromX) {
    var data = bmp.LockBits(new Rectangle(0, 0, bmp.Width, bmp.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
    var px = new byte[data.Stride * bmp.Height];
    Marshal.Copy(data.Scan0, px, 0, px.Length);
    bmp.UnlockBits(data);
    int x0 = bmp.Width, y0 = bmp.Height, x1 = -1, y1 = -1;
    for (int y = 0; y < bmp.Height; y++)
      for (int x = fromX; x < bmp.Width; x++)
        if (px[y * data.Stride + x * 4 + 3] > 25) { x0 = Math.Min(x0, x); y0 = Math.Min(y0, y); x1 = Math.Max(x1, x); y1 = Math.Max(y1, y); }
    return Rectangle.FromLTRB(x0, y0, x1 + 1, y1 + 1);
  }

  // `part` of `src` scaled into a w×h transparent canvas, centered, leaving `margin` of each side.
  public static Bitmap Fit(Bitmap src, Rectangle part, int w, int h, double margin) {
    var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb);
    double room = 1 - 2 * margin;
    double s = Math.Min(w * room / part.Width, h * room / part.Height);
    float dw = (float)(part.Width * s), dh = (float)(part.Height * s);
    using (var g = Graphics.FromImage(bmp)) {
      g.InterpolationMode = InterpolationMode.HighQualityBicubic;
      g.PixelOffsetMode = PixelOffsetMode.HighQuality;
      g.SmoothingMode = SmoothingMode.HighQuality;
      g.DrawImage(src, new RectangleF((w - dw) / 2, (h - dh) / 2, dw, dh), part, GraphicsUnit.Pixel);
    }
    return bmp;
  }
}
'@

# `from` with its background (the color of its top-left corner) keyed out and cropped to what is drawn
# from column fromX on, into a w×h PNG; w = 0 keeps the crop's own size plus the margin.
function Make([string]$from, [string]$to, [int]$w, [int]$h, [double]$margin, [int]$fromX = 0) {
  $src = [System.Drawing.Bitmap]::FromFile((Resolve-Path $from))
  $keyed = [BrandIcon]::KeyOut($src, $src.GetPixel(4, 4), 14, 70)
  $src.Dispose()
  $box = [BrandIcon]::Bounds($keyed, $fromX)
  if ($w -eq 0) { $w = [int][Math]::Ceiling($box.Width / (1 - 2 * $margin)); $h = [int][Math]::Ceiling($box.Height / (1 - 2 * $margin)) }
  $out = [BrandIcon]::Fit($keyed, $box, $w, $h, $margin)
  $out.Save($to, [System.Drawing.Imaging.ImageFormat]::Png)
  $keyed.Dispose(); $out.Dispose()
  Write-Output "brand-icon: $to ($w x $h)"
}

Make (Join-Path $brand 'aina-mark-original.png') (Join-Path $icons 'source.png') 1024 1024 $Margin
Make (Join-Path $brand 'aina-logo-original.png') (Join-Path $brand 'aina-logo.png') 0 0 0.02
Make (Join-Path $brand 'aina-logo-dark-original.png') (Join-Path $brand 'aina-logo-dark.png') 0 0 0.02
# The name alone in white (the dark logo right of its mark), for the sidebar.
$wordmark = Join-Path $env:TEMP 'aina-wordmark.png'
Make (Join-Path $brand 'aina-logo-dark-original.png') $wordmark 0 0 0 780

# The installer's images (NSIS wants 24-bit BMPs): the sidebar (164×314, the night-blue gradient with the
# mark, the white name and the tagline) and the header (150×57, the mark on white at the right).
$mark = [System.Drawing.Image]::FromFile((Resolve-Path (Join-Path $icons 'source.png')))
$hex = { param($h) [System.Drawing.ColorTranslator]::FromHtml($h) }

$side = New-Object System.Drawing.Bitmap 164, 314, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$g = [System.Drawing.Graphics]::FromImage($side)
$g.InterpolationMode = 'HighQualityBicubic'; $g.SmoothingMode = 'HighQuality'; $g.PixelOffsetMode = 'HighQuality'; $g.TextRenderingHint = 'AntiAliasGridFit'
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Point]::new(0, 0)), ([System.Drawing.Point]::new(0, 314)), ([System.Drawing.Color]::Black), ([System.Drawing.Color]::Black)
$blend = New-Object System.Drawing.Drawing2D.ColorBlend 4
$blend.Colors = @((& $hex '#080C15'), (& $hex '#142455'), (& $hex '#18317F'), (& $hex '#1E40AF'))
$blend.Positions = @(0.0, 0.32, 0.64, 1.0)
$grad.InterpolationColors = $blend
$g.FillRectangle($grad, 0, 0, 164, 314)
$g.DrawImage($mark, 30, 52, 104, 104) # the mark sits inside the 1024 square with an 8% margin
$name = [System.Drawing.Image]::FromFile($wordmark)
$nw = 100; $nh = [int]($name.Height * $nw / $name.Width)
$g.DrawImage($name, [int]((164 - $nw) / 2), 176, $nw, $nh)
$name.Dispose(); Remove-Item $wordmark
$center = New-Object System.Drawing.StringFormat; $center.Alignment = 'Center'
$fonts = New-Object System.Drawing.Text.PrivateFontCollection # Urbanist, the brand typeface
$fonts.AddFontFile((Resolve-Path (Join-Path $PSScriptRoot '..\..\assets\fonts\Urbanist-Variable.ttf')))
$tag = New-Object System.Drawing.Font($fonts.Families[0], 12, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$g.DrawString('Teman yang paham layar', $tag, (New-Object System.Drawing.SolidBrush (& $hex '#BFDBFE')), (New-Object System.Drawing.RectangleF(0, (186 + $nh), 164, 18)), $center)
$g.Dispose()
$side.Save((Join-Path $installer 'sidebar.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp); $side.Dispose()
Write-Output "brand-icon: installer\sidebar.bmp"

$head = New-Object System.Drawing.Bitmap 150, 57, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$g = [System.Drawing.Graphics]::FromImage($head)
$g.InterpolationMode = 'HighQualityBicubic'; $g.SmoothingMode = 'HighQuality'; $g.PixelOffsetMode = 'HighQuality'
$g.Clear([System.Drawing.Color]::White)
$g.DrawImage($mark, 96, 2, 53, 53)
$g.Dispose()
$head.Save((Join-Path $installer 'header.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp); $head.Dispose()
$mark.Dispose()
Write-Output "brand-icon: installer\header.bmp"

# The mark without the name, then the UI's copies (Settings shows the dark logo; sync-ui copies
# ui/assets/brand into ui-dist).
Copy-Item (Join-Path $icons 'source.png') (Join-Path $brand 'aina-mark.png')
$uiBrand = Join-Path $PSScriptRoot '..\ui\assets\brand'
New-Item -ItemType Directory -Force $uiBrand | Out-Null
Copy-Item (Join-Path $brand 'aina-mark.png'), (Join-Path $brand 'aina-logo.png'), (Join-Path $brand 'aina-logo-dark.png') $uiBrand
Write-Output "brand-icon: ..\assets\brand"
