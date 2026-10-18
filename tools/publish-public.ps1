# 公開リポジトリ（naktomo-svg/ClipEdit）に、今の版の中身を1コミットにまとめて写す。
# 開発は非公開のリポジトリ（ClipEdit-dev）で行い、配るときにだけこれを使う。公開側に作業の履歴・
# 作者のメールアドレスを出さないよう、コミットとタグは GitHub の noreply アドレスで作る。
# 使い方:
#   pwsh -NoProfile -File tools/publish-public.ps1          # 写してコミットとタグを作り、差分を見せる（push しない）
#   pwsh -NoProfile -File tools/publish-public.ps1 -Push    # 確かめたあとで push する（タグの push で Release が動く）
# 前提: 作業ツリーがきれいで、Cargo.toml の版と同じタグ（例: v0.2.0）があり、そのあとで exe に入るものを変えていないこと。
param(
    [switch]$Push,
    [string]$PublicDir = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'ClipEdit-public'),
    [string]$Repo = 'naktomo-svg/ClipEdit',
    [string]$AuthorName = 'nakto',
    [string]$AuthorEmail = '269830724+naktomo-svg@users.noreply.github.com'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (git status --porcelain) { throw '作業ツリーに変更が残っている。コミットしてから写す' }
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$tag = "v$version"
if (-not (git tag --list $tag)) { throw "タグ $tag がない。版を上げてコミットし、git tag -a $tag してから写す" }
# 公開するのは HEAD の中身。タグのあとで直してよいのは文書と道具だけで、exe に入るものは同じであること
$built = @('src', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'assets', '.cargo')
git diff --quiet $tag HEAD -- @built
if ($LASTEXITCODE -ne 0) { throw "タグ $tag のあとで exe に入るもの（$($built -join ', ')）が変わっている。版を上げてタグを付け直す" }
$source = git rev-parse --short HEAD

# 公開側の作業用の複製は、前回の残り（push していないコミットやタグ）が混ざらないよう毎回作り直す。
# 消すのは、このスクリプトが作った複製（origin が公開リポジトリ）のときだけ
$url = "https://github.com/$Repo.git"
if (Test-Path $PublicDir) {
    $existing = git -C $PublicDir remote get-url origin 2>$null
    if ($existing -ne $url) { throw "$PublicDir は公開リポジトリの複製ではないので消さない（origin: $existing）" }
    Remove-Item $PublicDir -Recurse -Force
}
git clone -q $url $PublicDir 2>$null
if ($LASTEXITCODE -ne 0) { throw "$url を複製できない" }
# 空のリポジトリを複製したときは、ブランチを master にしておく
git -C $PublicDir symbolic-ref HEAD refs/heads/master
if (git -C $PublicDir ls-remote --tags origin $tag) { throw "公開側にタグ $tag がもうある" }

# 中身を入れ替える（消したファイルも反映されるよう、.git 以外をいったん消してから展開する）
Get-ChildItem $PublicDir -Force | Where-Object Name -ne '.git' | Remove-Item -Recurse -Force
$zip = Join-Path ([IO.Path]::GetTempPath()) "clipedit-publish-$source.zip"
git archive --format=zip -o $zip HEAD
Expand-Archive $zip -DestinationPath $PublicDir
Remove-Item $zip

$id = @('-c', "user.name=$AuthorName", '-c', "user.email=$AuthorEmail")
git -C $PublicDir add -A
git -C $PublicDir @id commit -q -m "ClipEdit $tag" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git -C $PublicDir @id tag -a $tag -m "ClipEdit $tag"
"公開側に写した: $PublicDir（元: $source、タグ: $tag）"
git -C $PublicDir log --format='%h %an <%ae> %s' -1
git -C $PublicDir show --stat --format='' HEAD | Select-Object -Last 1

if ($Push) {
    git -C $PublicDir push -q origin master
    git -C $PublicDir push -q origin $tag
    "push した。タグの push で公開側の Release のワークフローが動く: https://github.com/$Repo/actions"
} else {
    '確かめてから -Push を付けて実行し直す（今の結果は公開側の作業用の複製に残っている）'
}
