# ClipEdit のアイコン (assets/clipedit.ico) を作り直すスクリプト。
# 使い方: pwsh -NoProfile -File tools/make-icon.ps1
# 各サイズを PNG で描き、PNG 形式の .ico にまとめる（Windows Vista 以降で読める形式）。
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$sizes = 16, 20, 24, 32, 40, 48
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root 'assets/clipedit.ico'

function New-RoundedRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $path
}

function Draw-Icon([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    $g.Clear([System.Drawing.Color]::Transparent)
    $s = $size / 32.0   # 32px を基準にした座標で描く

    $board = [System.Drawing.Color]::FromArgb(255, 37, 99, 175)
    $clip = [System.Drawing.Color]::FromArgb(255, 20, 50, 92)
    $line = [System.Drawing.Color]::FromArgb(255, 37, 99, 175)

    # 板
    $g.FillPath((New-Object System.Drawing.SolidBrush $board), (New-RoundedRect (3 * $s) (4 * $s) (26 * $s) (27 * $s) (3.5 * $s)))
    # 紙
    $g.FillPath([System.Drawing.Brushes]::White, (New-RoundedRect (7 * $s) (9 * $s) (18 * $s) (19 * $s) (1.5 * $s)))
    # クリップ
    $g.FillPath((New-Object System.Drawing.SolidBrush $clip), (New-RoundedRect (10 * $s) (1.5 * $s) (12 * $s) (6.5 * $s) (2 * $s)))
    # 文字の線（小さいサイズでは本数を減らして太くする）
    $brush = New-Object System.Drawing.SolidBrush $line
    if ($size -lt 24) {
        $g.FillRectangle($brush, [single](10 * $s), [single](14 * $s), [single](12 * $s), [single][Math]::Max(1, 2.5 * $s))
        $g.FillRectangle($brush, [single](10 * $s), [single](20 * $s), [single](8 * $s), [single][Math]::Max(1, 2.5 * $s))
    } else {
        foreach ($row in @(@(13, 12), @(17.5, 12), @(22, 8))) {
            $g.FillRectangle($brush, [single](10 * $s), [single]($row[0] * $s), [single]($row[1] * $s), [single](2 * $s))
        }
    }
    $g.Dispose()
    $bmp
}

$frames = foreach ($size in $sizes) {
    $bmp = Draw-Icon $size
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

$ms = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ms
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)   # ICONDIR
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {                                      # ICONDIRENTRY
    $w.Write([byte]$sizes[$i]); $w.Write([byte]$sizes[$i]); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$frames[$i].Length); $w.Write([uint32]$offset)
    $offset += $frames[$i].Length
}
foreach ($f in $frames) { $w.Write($f) }
$w.Flush()
[System.IO.File]::WriteAllBytes($out, $ms.ToArray())
"wrote $out ($($ms.Length) bytes)"
