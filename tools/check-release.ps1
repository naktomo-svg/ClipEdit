# リリース版 exe のセキュリティ設定を確かめるスクリプト（監査 S2 / S5 / S6）。
# 使い方: cargo build --release; pwsh -NoProfile -File tools/check-release.ps1
# Visual Studio（C++ ビルドツール）の dumpbin を使う。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target/release/clipedit.exe'

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products * -property installationPath
$dumpbin = Get-ChildItem (Join-Path $vs 'VC/Tools/MSVC') -Recurse -Filter dumpbin.exe |
    Where-Object FullName -like '*Hostx64*x64*' | Select-Object -First 1 -ExpandProperty FullName

$dependents = & $dumpbin /dependents $exe | Out-String
$headers = & $dumpbin /headers $exe | Out-String
$loadConfig = & $dumpbin /loadconfig $exe | Out-String
$bytes = [System.IO.File]::ReadAllBytes($exe)
$text = [System.Text.Encoding]::ASCII.GetString($bytes)
Add-Type -Namespace ClipEditCheck -Name Shell -MemberDefinition '[DllImport("shell32.dll", CharSet = CharSet.Unicode)] public static extern uint ExtractIconExW(string file, int index, System.IntPtr large, System.IntPtr small, uint count);'
# index -1 で、exe に入っているアイコンの数が返る
$iconCount = [ClipEditCheck.Shell]::ExtractIconExW($exe, -1, [IntPtr]::Zero, [IntPtr]::Zero, 0)
$version = & (Join-Path $PSScriptRoot 'cargo-version.ps1')
$info = (Get-Item $exe).VersionInfo

$checks = [ordered]@{
    # S2: VC++ ランタイム DLL を exe と同じフォルダから読まない（静的リンク）
    'S2 no VCRUNTIME dll'   = $dependents -notmatch 'VCRUNTIME|api-ms-win-crt'
    # S6: Control Flow Guard
    'S6 CFG enabled'        = $headers -match 'Control Flow Guard'
    'ASLR (dynamic base)'   = $headers -match 'Dynamic base'
    'DEP (NX compatible)'   = $headers -match 'NX compatible'
    # S5: ビルドした PC のユーザーフォルダのパスを含まない
    'S5 no user profile path' = -not $text.Contains($env:USERPROFILE)
    # S9: 起動時に読み込む DLL も System32 からだけ探す（/DEPENDENTLOADFLAG:0x800 = LOAD_LIBRARY_SEARCH_SYSTEM32）
    'S9 dependent load flag'  = $loadConfig -match '\b0*800 Dependent Load Flag'
    # D5: 画面部品を今の Windows の見た目にするマニフェスト（Common Controls v6）が入っている
    'D5 manifest embedded'    = $text.Contains('Microsoft.Windows.Common-Controls')
    # エクスプローラーに出す exe のアイコン（トレイと同じ assets/clipedit.ico）が入っている
    'exe icon embedded'       = $iconCount -ge 1
    # D39: エクスプローラーのプロパティで版が分かる（Cargo.toml の版と同じ）
    'D39 version info'        = $info.ProductVersion -eq $version -and $info.FileVersion -eq $version -and $info.ProductName -eq 'ClipEdit'
    # D73: 試験版（版に - がある）なら、エクスプローラーやインストーラーが正式版と見分けられるよう PRERELEASE の印を立てる
    'D73 pre-release flag'    = $info.IsPreRelease -eq $version.Contains('-')
}
$failed = 0
foreach ($c in $checks.GetEnumerator()) {
    '{0,-26} {1}' -f $c.Key, ($(if ($c.Value) { 'ok' } else { $failed++; 'FAILED' }))
}
exit $failed
