# tools/cargo-version.ps1 の検査（台帳 D74・D77・D78）。依存の version を [package] より前に書いた Cargo.toml でも、
# パッケージの版を読む。読めない書き方（version.workspace など）は、黙って違う版を返さずに失敗する。
# 使い方: pwsh -NoProfile -File tools/test-cargo-version.ps1
$ErrorActionPreference = 'Stop'
$dir = Join-Path ([IO.Path]::GetTempPath()) "clipedit-cargo-version-$PID"
New-Item -ItemType Directory -Force $dir | Out-Null
# 名前 → Cargo.toml の中身と、期待する版（$null は「失敗する」）
$cases = [ordered]@{
    'package first'      = @("[package]`nname = `"x`"`nversion = `"1.2.3`"`n`n[dependencies.windows]`nversion = `"0.62`"`n", '1.2.3')
    'dependency first'   = @("[dependencies.windows]`nversion = `"0.62`"`n`n[package]`nname = `"x`"`nversion = `"1.2.3`"`n", '1.2.3')
    'pre-release'        = @("[package]`nversion = `"1.2.3-beta.1`"`n", '1.2.3-beta.1')
    'CRLF'               = @("[dependencies.windows]`r`nversion = `"0.62`"`r`n[package]`r`nversion = `"1.2.3`"`r`n", '1.2.3')
    'header comment'     = @("[dependencies.windows] # Win32`nversion = `"0.62`"`n[package] # app`nversion = `"1.2.3`"`n", '1.2.3')
    'array of tables'    = @("[package]`nversion.workspace = true`n`n[[bin]]`nname = `"y`"`nversion = `"9.9.9`"`n", $null)
    'workspace version'  = @("[package]`nname = `"x`"`nversion.workspace = true`n[dependencies.windows]`nversion = `"0.62`"`n", $null)
}
$failed = 0
try {
    foreach ($c in $cases.GetEnumerator()) {
        $path = Join-Path $dir 'Cargo.toml'
        Set-Content -Path $path -Value $c.Value[0] -NoNewline
        $expected = $c.Value[1]
        # 読めないときは「version がない」で失敗すること。スクリプトが見つからないなどの別の失敗は、そのまま検査の失敗にする
        try { $got = & (Join-Path $PSScriptRoot 'cargo-version.ps1') -Path $path }
        catch { if ("$_" -like '*version がない*') { $got = $null } else { throw } }
        $ok = $got -eq $expected
        if (-not $ok) { $failed++ }
        '{0,-18} {1} ({2})' -f $c.Key, ($(if ($ok) { 'ok' } else { 'FAILED' })), ($(if ($null -eq $got) { '失敗した' } else { $got }))
    }
} finally {
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}
exit $failed
