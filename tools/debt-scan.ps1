# 技術負債の兆候を数え、基準値（docs/debt-baseline.json）との増減を出すスクリプト。
# 使い方:
#   pwsh -NoProfile -File tools/debt-scan.ps1                  # 計測して報告する
#   pwsh -NoProfile -File tools/debt-scan.ps1 -UpdateBaseline  # 今の値を基準値として保存する（測れなかった項目があれば保存しない）
#   pwsh -NoProfile -File tools/debt-scan.ps1 -Strict          # 悪化した項目か、測れなかった項目があれば終了コード 1
# 報告は target/debt/report-<日時>.md に保存する。台帳は docs/tech-debt.md。
param([switch]$UpdateBaseline, [switch]$Strict)
$ErrorActionPreference = 'Stop'
# cargo の出力は UTF-8。日本語を含む JSON を正しく読むために合わせる
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$baselinePath = Join-Path $root 'docs/debt-baseline.json'
$outDir = Join-Path $root 'target/debt'
New-Item -ItemType Directory -Force $outDir | Out-Null

# 値が大きいほど悪いもの（lower）と、参考の値（info）を分けて持つ
$metrics = [ordered]@{}
function Add-Metric($name, $value, $better, $label) {
    $metrics[$name] = [ordered]@{ value = $value; better = $better; label = $label }
}
$sources = Get-ChildItem src -Filter *.rs

# 1. clippy の厳しめの警告（種類ごと。bin とテストで重なる分は場所で重複を除く）
$seen = @{}
$byLint = @{}
cargo clippy --all-targets --message-format=json --quiet -- -W clippy::pedantic 2>$null | ForEach-Object {
    # JSON でない行（ビルドの進行表示など）は飛ばす
    $j = try { $_ | ConvertFrom-Json } catch { $null }
    if ($j -and $j.reason -eq 'compiler-message' -and $j.message.level -eq 'warning' -and $j.message.code) {
        $span = $j.message.spans | Where-Object is_primary | Select-Object -First 1
        $key = '{0}|{1}:{2}:{3}' -f $j.message.code.code, $span.file_name, $span.line_start, $span.column_start
        if (-not $seen.ContainsKey($key)) {
            $seen[$key] = $true
            $byLint[$j.message.code.code] = 1 + [int]$byLint[$j.message.code.code]
        }
    }
}
# ビルドに失敗すると警告が出ないまま終わるので、0 件と数えずに「測れず」にする（台帳 D33）
$clippyCount = if ($LASTEXITCODE -eq 0) { $seen.Count } else { $null }
Add-Metric 'clippy_pedantic' $clippyCount 'lower' 'clippy pedantic の警告'

# 2. unsafe
$unsafeBlocks = ($sources | Select-String -Pattern 'unsafe\s*\{').Count
$unsafeFns = ($sources | Select-String -Pattern 'unsafe fn ').Count
Add-Metric 'unsafe' ($unsafeBlocks + $unsafeFns) 'lower' 'unsafe（ブロックと関数）'

# 3. 後で直す印
$markers = @(Get-ChildItem src, tools -Recurse -File | Where-Object Name -ne 'debt-scan.ps1' |
    Select-String -Pattern '\b(TODO|FIXME|XXX|HACK)\b')
Add-Metric 'markers' $markers.Count 'lower' 'TODO / FIXME などの印'

# 4. ファイルの大きさ
$lines = @{}
foreach ($f in $sources) { $lines[$f.Name] = (Get-Content $f.FullName).Count }
$largest = $lines.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 1
Add-Metric 'largest_file_lines' $largest.Value 'lower' "いちばん大きいファイルの行数（$($largest.Name)）"
Add-Metric 'files_over_500' @($lines.Values | Where-Object { $_ -gt 500 }).Count 'lower' '500 行を超えるファイル'

# 5. テスト
$tests = ($sources | Select-String -Pattern '#\[test\]').Count
$ignored = ($sources | Select-String -Pattern '#\[ignore\b').Count
Add-Metric 'tests' $tests 'info' 'テストの数'
Add-Metric 'tests_ignored' $ignored 'lower' '手動でしか走らないテスト（#[ignore]）'

# 6. 依存ライブラリ（更新できるもの・重複・既知の脆弱性・cargo deny）
$updates = $null
try {
    $dry = cargo update --dry-run 2>&1 | Out-String
    $updates = ([regex]::Matches($dry, '(?m)^\s*(Updating|Upgrading) \S+ v\d')).Count
} catch { }
Add-Metric 'dep_updates' $updates 'info' '更新できる依存ライブラリ'

$dupNames = cargo tree -d --prefix none --format '{p}' 2>$null |
    ForEach-Object { if ($_ -match '^(\S+) v(\S+)') { "$($Matches[1])@$($Matches[2])" } } |
    Sort-Object -Unique | ForEach-Object { ($_ -split '@')[0] } | Group-Object | Where-Object Count -gt 1
Add-Metric 'dep_duplicates' @($dupNames).Count 'lower' "版が重複した依存ライブラリ（$(@($dupNames).Name -join ', ')）"

# cargo audit / cargo deny が入っていない・動かないときは、0 件と数えずに「測れず」にする（台帳 D33）。
# どちらも見つけたものがあると終了コードが 0 以外になるので、終了コードではなく出力の形で見分ける
$vulns = $null
try {
    $audit = cargo audit --json 2>$null | Out-String | ConvertFrom-Json
    # $null.count も 0 になるので、JSON に count があるかを確かめてから読む
    $count = $audit.vulnerabilities.PSObject.Properties['count']
    if ($count) { $vulns = [int]$count.Value }
} catch { }
Add-Metric 'vulnerabilities' $vulns 'lower' 'cargo audit の既知の脆弱性'

# 色付けすると「ok」の前に制御文字が入り、下の判定に合わなくなる（GitHub Actions で起きた）。
# 色付けを止め、念のため制御文字も取り除いてから見る
$denyOut = (cargo deny --color never check 2>&1 | Out-String) -replace "`e\[[0-9;]*m", ''
# 最後に「advisories ok, bans ok, licenses ok, sources ok」のように分類ごとの結果が出る
$denyResults = [regex]::Matches($denyOut, '\b(advisories|bans|licenses|sources) (ok|FAILED)\b')
$denyFailed = if ($denyResults.Count) { @($denyResults | Where-Object { $_.Groups[2].Value -eq 'FAILED' }).Count } else { $null }
if ($null -eq $denyFailed) {
    # 測れなかった理由を追えるよう、出力の最後を残す
    Write-Host "cargo deny の結果を読めませんでした。出力の最後:`n$(($denyOut -split "`n" | Select-Object -Last 15) -join "`n")"
}
Add-Metric 'deny_failed' $denyFailed 'lower' 'cargo deny で FAILED の分類'

# 7. ドキュメントとのずれ（ホットキーで使えるキーと、config.toml の上限）
$docs = @('README.md', 'docs/README-exe.md', 'config.example.toml')
$hotkeySrc = Get-Content src/hotkey.rs -Raw
$named = [regex]::Matches($hotkeySrc, '"([A-Z]{2,})" => Ok') | ForEach-Object { $_.Groups[1].Value }
$maxF = if ($hotkeySrc -match '\(1\.\.=(\d+)\)\.contains') { "F$($Matches[1])" } else { $null }
$sizeKb = if ((Get-Content src/config.rs -Raw) -match 'MAX_SIZE: u64 = (\d+) \* 1024') { $Matches[1] } else { $null }
$drift = @()
foreach ($d in $docs) {
    $text = Get-Content $d -Raw
    foreach ($k in @($named) + @($maxF)) {
        if ($k -and $text -notmatch "(?i)\b$k\b") { $drift += "$d に $k が書かれていない" }
    }
    if ($d -ne 'config.example.toml' -and $sizeKb -and $text -notmatch "$sizeKb\s?KB") {
        $drift += "$d に config.toml の上限 ${sizeKb}KB が書かれていない"
    }
}
# README のファイル構成表に、すべてのソース・ツール・ビルド設定が載っているか（台帳 D27）
$readme = Get-Content README.md -Raw
$listed = @(Get-ChildItem src -Filter *.rs | ForEach-Object { "src/$($_.Name)" }) +
    @(Get-ChildItem tools -File | ForEach-Object { "tools/$($_.Name)" }) +
    @('build.rs', 'deny.toml', 'assets/clipedit.manifest')
foreach ($path in $listed) {
    if ($readme -notmatch [regex]::Escape("``$path``")) { $drift += "README.md のファイル構成に $path が載っていない" }
}
Add-Metric 'doc_drift' $drift.Count 'lower' 'ドキュメントとコードのずれ'

# 基準値との比較
$baseline = if (Test-Path $baselinePath) { Get-Content $baselinePath -Raw | ConvertFrom-Json -AsHashtable } else { $null }
$worse = @()
$rows = foreach ($name in $metrics.Keys) {
    $m = $metrics[$name]
    $base = if ($baseline) { $baseline.metrics[$name] } else { $null }
    $mark = ''
    if ($null -eq $m.value -and $m.better -eq 'lower') {
        # 測れなかったものは、悪化していないと言えないので悪化と同じに扱う（台帳 D33）
        $mark = '✕ 測れず'; $worse += "$($m.label)（測れず）"
    } elseif ($null -ne $m.value -and $null -ne $base -and $m.better -eq 'lower') {
        if ($m.value -gt $base) { $mark = '▲ 悪化'; $worse += $m.label }
        elseif ($m.value -lt $base) { $mark = '▽ 改善' }
    }
    $val = if ($null -eq $m.value) { '測れず' } else { $m.value }
    $b = if ($null -eq $base) { '—' } else { $base }
    "| $($m.label) | $val | $b | $mark |"
}
$lintRows = foreach ($l in $byLint.GetEnumerator() | Sort-Object Value -Descending) {
    $base = if ($baseline -and $baseline.clippy_by_lint.ContainsKey($l.Key)) { $baseline.clippy_by_lint[$l.Key] } else { 0 }
    $mark = if ($baseline -and $l.Value -gt $base) { '▲' } else { '' }
    "| $($l.Key) | $($l.Value) | $base | $mark |"
}

$stamp = Get-Date -Format 'yyyy-MM-dd HH:mm'
$commit = git rev-parse --short HEAD 2>$null
$report = @"
# 技術負債の兆候（$stamp、$commit）

基準値: $(if ($baseline) { "$($baseline.date)（$($baseline.commit)）" } else { 'なし（-UpdateBaseline で作る）' })

| 兆候 | 今 | 基準値 | 増減 |
|---|---|---|---|
$($rows -join "`n")

## clippy pedantic の内訳

| lint | 今 | 基準値 | |
|---|---|---|---|
$($lintRows -join "`n")

## 印（TODO など）

$(if ($markers.Count) { ($markers | ForEach-Object { "- $($_.Path | Resolve-Path -Relative):$($_.LineNumber) $($_.Line.Trim())" }) -join "`n" } else { 'なし' })

## ドキュメントとのずれ

$(if ($drift.Count) { ($drift | ForEach-Object { "- $_" }) -join "`n" } else { 'なし' })

## 悪化した兆候

$(if ($worse.Count) { ($worse | ForEach-Object { "- $_" }) -join "`n" } else { 'なし' })
"@
$reportPath = Join-Path $outDir ("report-{0}.md" -f (Get-Date -Format 'yyyyMMdd-HHmm'))
Set-Content -Path $reportPath -Value $report -Encoding utf8
$report
"`n報告: $reportPath"

$unmeasured = @($metrics.Keys | Where-Object { $null -eq $metrics[$_].value -and $metrics[$_].better -eq 'lower' })
if ($UpdateBaseline -and $unmeasured.Count) {
    # 測れなかった値を基準値にすると、次からその項目を比べられなくなる（台帳 D33）
    "測れなかった項目があるので、基準値を更新しません: $($unmeasured -join ', ')"
    exit 1
}
if ($UpdateBaseline) {
    $values = [ordered]@{}
    foreach ($name in $metrics.Keys) { $values[$name] = $metrics[$name].value }
    $lintMap = [ordered]@{}
    foreach ($l in $byLint.GetEnumerator() | Sort-Object Name) { $lintMap[$l.Key] = $l.Value }
    [ordered]@{ date = (Get-Date -Format 'yyyy-MM-dd'); commit = $commit; metrics = $values; clippy_by_lint = $lintMap } |
        ConvertTo-Json -Depth 4 | Set-Content -Path $baselinePath -Encoding utf8
    "基準値を更新しました: $baselinePath"
}
if ($Strict -and $worse.Count) { exit 1 }
