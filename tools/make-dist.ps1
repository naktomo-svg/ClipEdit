# 配布用の zip を作るスクリプト。
# 使い方: pwsh -NoProfile -File tools/make-dist.ps1
# build-release.ps1 で exe を作って検査し、target/dist/ClipEdit/ に配布するファイルをそろえて
# target/dist/ClipEdit-<版>-win64.zip にまとめ、target/dist/SHA256SUMS.txt も書く。版は Cargo.toml の version。
# 常駐中の ClipEdit が target/release/clipedit.exe を使っていると上書きできないので、先に止めておく。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

pwsh -NoProfile -File (Join-Path $PSScriptRoot 'build-release.ps1')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root 'target/dist'
$folder = Join-Path $dist 'ClipEdit'
$zip = Join-Path $dist "ClipEdit-$version-win64.zip"

# 前回の残りが混ざらないよう、毎回作り直す
if (Test-Path $folder) { Remove-Item $folder -Recurse -Force }
New-Item -ItemType Directory -Force $folder | Out-Null
# 配布物の名前 = リポジトリの中の元のファイル
$files = [ordered]@{
    'clipedit.exe'        = 'target/release/clipedit.exe'
    'README.md'           = 'docs/README-exe.md'
    'config.example.toml' = 'config.example.toml'
    'LICENSE'             = 'LICENSE'
}
foreach ($f in $files.GetEnumerator()) {
    Copy-Item (Join-Path $root $f.Value) (Join-Path $folder $f.Key)
}
# お気に入りの見本（展開してすぐ Ctrl+1 で試せるように、exe の隣の favorites に入れる）
$samples = @(Get-ChildItem (Join-Path $root 'assets/favorites') -Filter *.txt)
if ($samples.Count -eq 0) { throw 'assets/favorites にお気に入りの見本がありません' }
$favorites = Join-Path $folder 'favorites'
New-Item -ItemType Directory -Force $favorites | Out-Null
$samples | Copy-Item -Destination $favorites
# exe に入っている依存ライブラリの著作権表示とライセンス文（台帳 D32）
pwsh -NoProfile -File (Join-Path $PSScriptRoot 'make-notices.ps1') -Out (Join-Path $folder 'THIRD-PARTY-NOTICES.txt')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $folder -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
# 利用者がダウンロードしたものを確かめるためのチェックサム（sha256sum と同じ形式。監査 S17）。
# 展開したあとの exe も確かめられるよう、exe の分も書く
$sums = Join-Path $dist 'SHA256SUMS.txt'
$exeHash = (Get-FileHash (Join-Path $folder 'clipedit.exe') -Algorithm SHA256).Hash
# sha256sum -c は CRLF の \r をファイル名の一部として読むので、行末は LF にする
$lines = @(
    "$($hash.ToLower())  $(Split-Path $zip -Leaf)"
    "$($exeHash.ToLower())  ClipEdit/clipedit.exe"
)
[IO.File]::WriteAllText($sums, ($lines -join "`n") + "`n", [Text.Encoding]::ASCII)
"zip:    $zip"
"SHA256: $hash"
"チェックサム: $sums"
