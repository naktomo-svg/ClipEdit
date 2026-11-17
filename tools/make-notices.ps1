# exe に入る依存ライブラリの著作権表示とライセンス文を、1つのテキストファイルにまとめる（台帳 D32）。
# 使い方: pwsh -NoProfile -File tools/make-notices.ps1 -Out target/dist/ClipEdit/THIRD-PARTY-NOTICES.txt
# exe に入るのは、通常の依存（dev・build の依存と、ビルド中にだけ動く proc-macro とその依存を除く）と Rust の標準ライブラリ。
# どれも「MIT OR Apache-2.0」か MIT なので、MIT のライセンス文を使う。MIT を選べないものがあれば止まる。
param([Parameter(Mandatory)][string]$Out)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent $PSScriptRoot
$target = 'x86_64-pc-windows-msvc'

$manifest = Join-Path $root 'Cargo.toml'
# ライセンス文のファイルの場所を知るために使う。cargo metadata の依存の一覧は機能（features）を広めに解決して、
# 実際には入らないライブラリも含むので、入るものの一覧は cargo tree で決める
$meta = cargo metadata --locked --format-version 1 --manifest-path $manifest | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$packages = @{}
foreach ($p in $meta.packages) { $packages["$($p.name) v$($p.version)"] = $p }

# 通常の依存だけをたどる。proc-macro はビルド中にだけ動き exe に入らないので、その下の行も飛ばす
$tree = cargo tree --locked --manifest-path $manifest -e normal --target $target --prefix depth --format '{p}'
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$linked = [ordered]@{}
$skipBelow = [int]::MaxValue
foreach ($line in $tree) {
    if ($line -notmatch '^(\d+)(\S+ v\S+)(.*)$') { continue }
    $depth = [int]$Matches[1]
    $key = $Matches[2]
    $rest = $Matches[3]
    if ($depth -gt $skipBelow) { continue }
    $skipBelow = [int]::MaxValue
    if ($rest -match '\(proc-macro\)') { $skipBelow = $depth; continue }
    if ($depth -eq 0 -or $linked.Contains($key)) { continue }
    $linked[$key] = $packages[$key] ?? $(throw "cargo metadata に $key がない")
}

# 同じライセンス文のライブラリはまとめて1回だけ書く（windows-* などは同じ文）
$groups = [ordered]@{}
foreach ($p in $linked.Values | Sort-Object name, version) {
    if ($p.license -notmatch '\bMIT\b' -or $p.license -match '\bAND\b') {
        throw "$($p.name) $($p.version) のライセンス「$($p.license)」は MIT を選べない。make-notices.ps1 を見直す"
    }
    $dir = Split-Path $p.manifest_path
    $file = Get-ChildItem $dir -File | Where-Object Name -match '^licen[cs]e[-_.]?mit' | Select-Object -First 1
    if (-not $file) { throw "$($p.name) $($p.version) に MIT のライセンス文のファイルがない（$dir）" }
    $text = ((Get-Content $file.FullName) | ForEach-Object TrimEnd) -join "`r`n"
    $text = $text.Trim()
    if (-not $groups.Contains($text)) { $groups[$text] = @() }
    $groups[$text] += "$($p.name) $($p.version)（$($p.license)）$(if ($p.repository) { " $($p.repository)" })"
}

# Rust の標準ライブラリ。中に入っているライブラリの一覧は、Rust の share/doc/rust/COPYRIGHT-library.html にある
$sysroot = rustc --print sysroot
$mit = (Get-Content (Join-Path $sysroot 'share/doc/rust/licenses/MIT.txt')) -join "`r`n"
$std = $mit.Replace('Copyright (c) <year> <copyright holders>', 'Copyright (c) The Rust Project Contributors').Trim()
$rustVersion = (rustc --version) -replace '^rustc (\S+).*', '$1'

$rule = '=' * 72
$body = @(
    'ClipEdit が使っているライブラリの著作権表示とライセンス'
    ''
    'clipedit.exe には、次のライブラリが含まれています。'
    'どれも MIT License（一部は MIT License と Apache License 2.0 からの選択）で配布されており、'
    'ClipEdit は MIT License を選んで使っています。'
    ''
    $rule
    "Rust の標準ライブラリ（Rust $rustVersion。MIT OR Apache-2.0） https://github.com/rust-lang/rust"
    $rule
    ''
    $std
    ''
)
foreach ($g in $groups.GetEnumerator()) {
    $body += $rule
    $body += $g.Value
    $body += $rule
    $body += ''
    $body += $g.Key
    $body += ''
}
New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null
# メモ帳で開いても崩れないよう CRLF にする
[System.IO.File]::WriteAllText($Out, ($body -join "`r`n"), [System.Text.UTF8Encoding]::new($false))
"$($linked.Count) 個のライブラリと標準ライブラリを書きました: $Out"
