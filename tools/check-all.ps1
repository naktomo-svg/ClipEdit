# すべての検査を1つのコマンドで流す（リモートの CI がない間の代わり。台帳 D12・D13）。
# 使い方: pwsh -NoProfile -File tools/check-all.ps1          # 書式・clippy・テスト・負債の検査・ライセンス文の書き出し
#         pwsh -NoProfile -File tools/check-all.ps1 -Hardware # 実機のクリップボード・画面・レジストリを使うテストと、配布用ビルドの検査も流す
# -Hardware はクリップボードの中身を書き換え、ウィンドウを出し、常駐中の ClipEdit を止める。
param([switch]$Hardware)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$results = [ordered]@{}
function Step($name, [scriptblock]$body) {
    Write-Host "== $name" -ForegroundColor Cyan
    & $body
    $results[$name] = if ($LASTEXITCODE -eq 0) { 'ok' } else { 'FAILED' }
}

Step '書式（cargo fmt --check）' { cargo fmt --check }
Step 'clippy' { cargo clippy --locked --all-targets --quiet -- -D warnings }
Step 'テスト' { cargo test --locked --quiet }
Step '負債の兆候（debt-scan -Strict）' { pwsh -NoProfile -File tools/debt-scan.ps1 -Strict | Select-Object -Last 3 }
# 配布の直前ではなく、依存を変えたときに気づけるよう、毎回書き出してみる（台帳 D43）
# どのケースが落ちたか分かるよう、結果はそのまま出す（台帳 D77）
Step '版の読み取り（test-cargo-version）' { pwsh -NoProfile -File tools/test-cargo-version.ps1 }
Step 'ライセンス文（make-notices）' { pwsh -NoProfile -File tools/make-notices.ps1 -Out target/notices/THIRD-PARTY-NOTICES.txt }
if ($Hardware) {
    Get-Process clipedit -ErrorAction SilentlyContinue | Stop-Process -Force
    Step '実機のテスト（--ignored）' { cargo test --locked --quiet -- --ignored --test-threads=1 }
    Step '配布用ビルドと exe の検査' { pwsh -NoProfile -File tools/build-release.ps1 }
}

Write-Host ''
$failed = 0
foreach ($r in $results.GetEnumerator()) {
    '{0,-34} {1}' -f $r.Key, $r.Value
    if ($r.Value -ne 'ok') { $failed++ }
}
if (-not $Hardware) { '（実機のテストと配布用ビルドは -Hardware を付けると流れます）' }
exit $failed
