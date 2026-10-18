# ClipEdit

クリップボードのテキストを、ショートカットキーひとつで開いて編集できる Windows 用の常駐アプリです。
編集後に Ctrl+Enter を押すとクリップボードに書き戻されるので、あとは貼り付けたい場所で Ctrl+V を押します。

- タスクトレイに常駐し、ホットキー（既定は **Ctrl+Alt+V**）ですぐ開く（もう一度 exe を起動しても、動いているほうの編集ウィンドウが開きます）
- Rust と Win32 API だけで作った約 500KB の exe 1つ（インストール不要、ランタイム不要）
- ネットワーク通信なし、ログ出力なし

## 使い方

1. `clipedit.exe` を起動すると、タスクトレイにアイコンが出ます。
   Windows 11 では最初は「^」の中に入っているので、タスクバーへドラッグすると常に表示されます。
2. テキストをコピーしてから **Ctrl+Alt+V** を押すと、マウスカーソルのある画面の中央に編集ウィンドウが開きます。
3. 編集して **Ctrl+Enter** を押すと、クリップボードが書き換わってウィンドウが閉じます。
4. 貼り付けたい場所で **Ctrl+V** を押します。

| キー | 動作 |
|---|---|
| Ctrl+Enter | クリップボードに書き込んで閉じる |
| Esc / × ボタン | 書き込まずに閉じる |
| Ctrl+A | すべて選択 |
| Ctrl+Backspace | 直前の1語を消す（日本語はひらがな・カタカナ・漢字の区切りで消える） |
| Ctrl+Z | 元に戻す |

- 開いている間にもう一度ホットキーを押しても、編集中の内容は消えません（前面に出るだけ）。
- クリップボードが画像などテキスト以外のときは、空の状態で開き、タイトルに「(テキストなし)」と出ます。
- 改行はすべて CRLF にそろえて表示・書き込みします。
- 5,000 文字を超える長い行（改行のない JSON や Base64 など）があるときは、折り返さずに横スクロールで表示し、タイトルに「(長い行があるため折り返さずに表示)」と出ます（折り返すと開くのに何十秒もかかるため）。

### トレイアイコン

- 左クリック: 編集ウィンドウを開く
- 右クリック: メニュー
  - **開く**
  - **設定を読み直す**: `config.toml` を読み直し、ホットキーを登録し直します（書き方が違う・新しいキーがほかのアプリに使われているときは、それまでのホットキーをそのまま使います）
  - **ログイン時に起動**: チェックすると Windows へのサインイン時に自動で起動します（`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` に登録）。exe を移動したときはチェックが外れた表示になるので、もう一度選ぶと新しい場所で登録し直します。
  - **終了**

## 設定

`config.example.toml` を `config.toml` という名前で `clipedit.exe` と同じフォルダに置くと、ホットキーを変えられます。
UTF-8 で保存してください。変更は、トレイのメニューの「設定を読み直す」を選ぶと反映されます（起動し直しても反映されます）。

```toml
hotkey = "Ctrl+Shift+E"
```

- 修飾キー: `Ctrl` / `Alt` / `Shift` / `Win`（1つ以上必要、大文字小文字は区別しない）
- キー: `A`〜`Z`、`0`〜`9`、`F1`〜`F24`、`Space`、`Insert`

ファイルが読めない・書き方が違う・64KB を超える、といった場合は既定値（Ctrl+Alt+V）で起動し、トレイから通知で知らせます。
ホットキーがほかのアプリに使われているときは、ホットキーなしで起動して通知を出します。その間もトレイのアイコンから開けます。

## セキュリティとプライバシー

- パスワード管理ソフトなどが「クリップボード履歴・クラウドに残さない」印を付けてコピーしたテキストは、書き戻すときにも同じ印を付け直します。ClipEdit を通しても Win+V の履歴やクラウド同期に残りません。
- ウィンドウを閉じると、編集欄の内容と「元に戻す」の履歴を消します。
- C ランタイムを静的リンクしているので、exe と同じフォルダに置かれた DLL を読み込みません。
- 詳しくは [docs/security-audit-2026-10-15.md](docs/security-audit-2026-10-15.md) を参照してください。

## ビルド

必要なもの: Rust（stable、2024 edition）、Visual Studio の C++ ビルドツール（MSVC）

```powershell
# 配布用（ユーザー名などビルドした PC のパスを exe から除き、保護設定を検査する）
pwsh -NoProfile -File tools/build-release.ps1

# 開発用
cargo build --release
```

できた exe は `target\release\clipedit.exe` です。
ClipEdit が起動していると exe を上書きできないので、ビルド前にトレイの「終了」で止めてください。

`.cargo/config.toml` で C ランタイムの静的リンクと Control Flow Guard を有効にしています。
`tools/check-release.ps1` は、ビルドした exe がこれらの設定を満たすかを確かめます。

## テスト

```powershell
cargo test                                   # 通常のテスト
cargo test -- --ignored --test-threads=1     # 実際のクリップボード・ウィンドウ・レジストリを使うテスト
cargo clippy --all-targets                   # unsafe ブロックには SAFETY コメントが必須
```

まとめて流すときは `pwsh -NoProfile -File tools/check-all.ps1` を使います（書式・clippy・テスト・負債の検査・ライセンス文の書き出し）。
`-Hardware` を付けると、実機を使うテストと配布用ビルドの検査も流します。

GitHub に push するかプルリクエストを出すと、GitHub Actions（`.github/workflows/ci.yml`）が `check-all.ps1` と配布用ビルドの検査を流します。実機を使うテストは CI では流さないので、手元で `-Hardware` を付けて流してください。

配布するときは、`Cargo.toml` の `version` を上げてコミットし、同じ版のタグ（例: `v0.2.0`）を push します。`.github/workflows/release.yml` が検査・ビルドをして、zip とチェックサムを GitHub Release に上げます（タグと `Cargo.toml` の版が違うと止まります）。

開発は非公開のリポジトリ `naktomo-svg/ClipEdit-dev` で行い、一般に配るのは公開リポジトリ `naktomo-svg/ClipEdit` です。公開側には作業の履歴と作者のメールアドレスを出さないため、タグを付けたあとで `pwsh -NoProfile -File tools/publish-public.ps1` を実行し、差分を確かめてから `-Push` を付けて実行し直します。公開側では、タグの push で同じ `release.yml` が動き、attestation 付きの Release ができます。

`--ignored` のテストはクリップボードの中身を書き換えます（レジストリの自動起動設定はテスト後に元に戻します）。

## ファイル構成

| パス | 内容 |
|---|---|
| `src/main.rs` | 起動・トレイとホットキーのメッセージ処理・設定の読み直し |
| `src/instance.rs` | 2回目に起動されたとき、動いているほうに編集ウィンドウを開かせる |
| `src/editor.rs` | 編集ウィンドウ |
| `src/clipboard.rs` | クリップボードの読み書き |
| `src/hotkey.rs` | ホットキー文字列の解析と登録、設定を読み直したときのキーの切り替え |
| `src/config.rs` | `config.toml` の読み込み |
| `src/tray.rs` | トレイアイコン・通知・メニュー |
| `src/autostart.rs` | ログイン時の自動起動 |
| `src/wide.rs` | Win32 に渡す 0 終端の UTF-16 と文字列の変換 |
| `assets/clipedit.ico` | アイコン（`tools/make-icon.ps1` で生成し、exe に埋め込み） |
| `assets/clipedit.manifest` | アプリのマニフェスト（`build.rs` が exe に埋め込む） |
| `build.rs` | マニフェストとアイコンを exe に埋め込むビルドスクリプト |
| `deny.toml` | `cargo deny check` の設定（使ってよいライセンス） |
| `tools/build-release.ps1` | 配布用のビルド（最後に `tools/check-release.ps1` で exe を検査） |
| `tools/make-dist.ps1` | 配布用の zip を作る（`build-release.ps1` でビルドと検査をしてから、`target/dist/` にまとめる） |
| `tools/publish-public.ps1` | 公開リポジトリ（`naktomo-svg/ClipEdit`）に、今の版の中身を noreply のアドレスで1コミットにまとめて写す（開発は非公開の `ClipEdit-dev`） |
| `tools/make-notices.ps1` | exe に入る依存ライブラリの著作権表示とライセンス文を `THIRD-PARTY-NOTICES.txt` にまとめる（`make-dist.ps1` が使う） |
| `tools/check-all.ps1` | すべての検査をまとめて流す |
| `.github/workflows/ci.yml` | GitHub Actions の CI（Rust 1.99.0 に固定。外部の Action はコミットのハッシュで固定） |
| `.github/workflows/release.yml` | `v` で始まるタグを push すると、配布用 zip・`SHA256SUMS.txt`・作った場所の証明（artifact attestation。公開リポジトリのときだけ）を GitHub Release に上げる |
| `tools/debt-scan.ps1` | 技術負債の兆候を数え、基準値と比べる（台帳は `docs/tech-debt.md`） |
| `docs/` | 設計書・実装計画・セキュリティ監査報告・負債の台帳・配布前に手で確かめる手順・配布用 README |

## 動作環境

Windows 10（バージョン 1703 以降）/ Windows 11、64bit

## ライセンス

[MIT License](LICENSE)
