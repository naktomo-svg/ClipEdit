# Cargo.toml の [package] の version を出す（台帳 D74）。依存の version（[dependencies.windows] など）は読まない。
# 使い方: $version = & tools/cargo-version.ps1            # このリポジトリの Cargo.toml
#         $version = & tools/cargo-version.ps1 -Path x.toml
param([string]$Path = (Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml'))
$ErrorActionPreference = 'Stop'
$section = ''
foreach ($line in Get-Content -LiteralPath $Path) {
    # 見出しの後ろのコメント（[package] # …）は読み飛ばす（台帳 D78）。[[bin]] のような表の配列も見出しとして読む
    if ($line -match '^\s*\[\[?([^\]]+)\]\]?\s*(#.*)?$') { $section = $Matches[1].Trim(); continue }
    if ($section -eq 'package' -and $line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
}
throw "$Path の [package] に version がない"
