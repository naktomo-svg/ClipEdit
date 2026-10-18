# 配布用のリリース版 exe を作るスクリプト。
# 使い方: pwsh -NoProfile -File tools/build-release.ps1
# 監査 S5: exe に残るソースのパスから、ビルドした PC のユーザーフォルダ（ユーザー名を含む）を "~" に置き換える。
# 静的リンクと CFG は .cargo/config.toml で指定している（--config の rustflags はそれに追加される）。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$remap = "--remap-path-prefix=$env:USERPROFILE=~"
cargo build --release --manifest-path (Join-Path $root 'Cargo.toml') `
    --config "target.x86_64-pc-windows-msvc.rustflags=['$remap']"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
pwsh -NoProfile -File (Join-Path $PSScriptRoot 'check-release.ps1')
exit $LASTEXITCODE
