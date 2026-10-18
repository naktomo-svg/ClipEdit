# ClipEdit セキュリティ監査報告（2026-10-15）

対象: master `d4f9744`、`target\release\clipedit.exe`
前提: ネットワーク通信なし。ログイン中ユーザーの権限で動く常駐アプリ。評価は「自分専用」で行い、配布する場合に重要度が上がるものは併記した。

## 指摘一覧

| ID | 重要度 | 内容 | 確認方法 |
|---|---|---|---|
| S1 | **高** | パスワード管理ソフトが付ける「履歴・クラウドに残さない」印が、ClipEdit で書き込むと消える | 実機で再現 |
| S2 | 中（配布するなら高） | exe と同じフォルダの `vcruntime140.dll` を読み込む（DLL 差し替え） | 実機で再現 |
| S3 | 中 | 閉じた後も編集欄に前のテキストが残り、他のアプリから読める | 実機で再現 |
| S4 | 低 | config.toml の大きさに上限がない（100MB で約 509MB のメモリを使う） | 実機で確認 |
| S5 | 低（配布するなら中） | exe にユーザー名入りのパス（`C:\Users\<ユーザー名>\...`）が 14 か所入っている | exe を検索 |
| S6 | 低 | Control Flow Guard (CFG) が無効 | dumpbin |
| S7 | 情報 | 自動起動の ON/OFF 表示が、登録パスが今の exe かを見ていない | コードを読む |
| S8 | 情報 | 単一起動の判定を、同じユーザーの別アプリに横取りされると起動できない | コードを読む |

### S1 履歴除外の印が消える（高）
- **内容**: KeePass や 1Password などは、パスワードをコピーするときに `ExcludeClipboardContentFromMonitorProcessing`、`CanIncludeInClipboardHistory`、`CanUploadToCloudClipboard` という形式も一緒に置く。これがあると、Windows のクリップボード履歴（Win+V）やクラウド同期、他のクリップボード管理ソフトは、その内容を記録しない。`clipboard::write_text` は `EmptyClipboard` のあとテキストだけを置くため、これらの印が消える。
- **再現**: 3つの印を付けたテキストを置き、ホットキーで開いてから Ctrl+Enter を押す。書き込む前はある印が、書き込んだ後は3つとも無い（`audit/dyn.ps1` の項目1）。
- **影響**: パスワードをコピーし、ClipEdit で開いて Ctrl+Enter を押すと、そのパスワードが履歴やクラウド、クリップボード管理ソフトに残る。このPCでは `EnableClipboardHistory` が設定されていないので、今は履歴がオフの可能性が高い。ただし履歴をオンにした時点で実害が出る。他のクリップボード管理ソフトには、履歴の設定と関係なく影響する。
- **対策**: `read_text` で印の有無も返し、印が付いていたテキストを書き込むときは同じ印を付け直す。具体的には除外の形式を置き、履歴とクラウドの形式には DWORD の 0 を置く。

### S2 DLL 差し替え（中）
- **内容**: exe は `VCRUNTIME140.dll` を動的に読み込む。これは KnownDLLs に入っていないので、Windows はまず exe と同じフォルダを探す。
- **再現**: exe をコピーしたフォルダに偽の `vcruntime140.dll` を置いて起動すると、「clipedit.exe - 正しくないイメージ」が出る。つまり偽物が読み込まれた。
- **影響**: exe を「ダウンロード」など他人やブラウザが書き込めるフォルダに置くと、そこに置かれた DLL が ClipEdit の権限で動く。自動起動を登録していれば、ログインのたびに動く。今の置き場所（ユーザーフォルダの中）は本人しか書き込めないので、自分専用なら影響は小さい。
- **対策**: CRT を静的リンクする（`.cargo/config.toml` に `-C target-feature=+crt-static`）。これで VC++ 再頒布パッケージが入っていない PC でも起動できるようになる。

### S3 閉じた後のテキスト残留（中）
- **内容**: Esc や Ctrl+Enter で閉じても、隠れた編集欄にテキストが残る。同じユーザーで動く他のアプリは `WM_GETTEXT` で読める。元に戻す（Ctrl+Z）用のバッファにも残る。
- **再現**: `secret-residue-123` を開いて Esc で閉じ、クリップボードを別の内容に変えたあとで編集欄を読むと、`secret-residue-123` が返る（`audit/residue.ps1`）。
- **影響**: 閉じたつもりのパスワードが、次に開くまでメモリと画面部品の中に残る。ただし同じユーザーのアプリは、そもそもクリップボードも読めるので、増えるリスクは限られる。
- **対策**: `hide` で `SetWindowTextW(edit, "")` と `EM_EMPTYUNDOBUFFER` を実行する。開くときはいつもクリップボードを読み直すので、使い勝手は変わらない。

### S4 config.toml の大きさ（低）
NUL で埋めた 100MB のファイルでも、起動して既定値で動いた。ただし最大で約 509MB のメモリを使った。読む前に大きさを確かめ、64KB を超えたら既定値と警告にする。

### S5 exe 内のユーザー名（低）
パニックしたときに出すメッセージ用のパス（`C:\Users\<ユーザー名>\.cargo\registry\...`、`.rustup\...`）が 14 か所ある。配布するなら、`--remap-path-prefix` でパスを置き換える。

### S6 CFG 無効（低）
ASLR（高エントロピー）と DEP は有効、CFG は無効だった。`-C control-flow-guard` で有効にできる。ただし標準ライブラリは CFG 付きでビルドされていないので、効果は限られる。

### S7 / S8（情報）
- S7: exe を移動しても、Run キーには古いパスが残り、メニューにはチェックが付いたままになる。`is_enabled` が値の中身と今の exe を比べれば直る。
- S8: `Local\ClipEdit-SingleInstance` を先に作られると、ClipEdit が起動しない。ただし起動を妨げるだけで、それができるのは同じユーザーのアプリに限られる。対策は不要と判断した。

## 問題なしだった項目
- **依存ライブラリ**:
  - `cargo audit` で、29 crate に既知の脆弱性はなかった。
  - `cargo deny` の advisories / bans / sources は ok。ライセンスはすべて MIT / Apache-2.0 / Unicode-3.0 だった。licenses が FAILED になったのは、許可するライセンスを決めた設定ファイルを用意せずに実行したため。
  - clipedit 自身に `license` 欄が無い。配布しないなら `publish = false` で足りる。
  - `syn` 2 と 3 の重複は無害。`Cargo.lock` はコミット済み。
- **exe の設定**:
  - ASLR / 高エントロピー ASLR / DEP は有効。
  - マニフェストが無いので、権限は起動した人と同じ（asInvoker）。管理者権限は求めない。
- **自動起動**: Run キーのパスは引用符で囲んである。
- **情報漏れ**:
  - ログやデバッグ出力は無い（`println!` / `dbg!` 等は 0 件）。
  - 通知やタイトルにクリップボードの中身は出ない。
- **`unsafe` の全件レビュー（46 ブロック）**:
  - 境界の誤りや解放漏れは見つからなかった。
  - `read_text` は `GlobalSize` で読む範囲を抑えている。
  - `text()` のバッファは長さ+1 で確保している。
  - Ctrl+Backspace はキャレットを長さで切り詰めている。
  - `EM_GETSEL` の受け取り先は u32。
  - フォントは古いものを確実に解放している。
- **細工したメッセージ**: 不正なポインタを入れた `WM_DPICHANGED` を他のプロセスから送っても、落ちなかった。
- **ランダム入力（proptest、12 種類で計約 30 万件）**:
  - 対象: ホットキーの解析、config の解析、深く入れ子にした TOML（20 万段）、アイコンの解析、`copy_wide`、`text_until_nul`、`prev_word_start`、改行の統一、`center_rect`。
  - いずれもパニックせず、性質も成り立った。
- **clippy の厳しめの設定（pedantic）**:
  - 不具合につながる警告は無かった。
  - 型変換の警告は、どれも値の範囲から見て問題ない。
  - SAFETY コメントの無い `unsafe` が 46 か所ある。保守のための推奨事項にとどめる。

## 推奨する修正順
1. S1（パスワードが履歴に残る）
2. S3（閉じた後の残留）
3. S2（CRT の静的リンク。配布にも役立つ）
4. S4 → S7 → S5 / S6

## 対応結果（ブランチ fix/security-audit）

| ID | 状態 | 対応 | 確認 |
|---|---|---|---|
| S1 | 解消 | `read_text` が印の有無（`ClipText::private`）も返す。`write_text` は印が付いていたテキストを書き込むとき、3つの印を付け直す | `private_text_keeps_exclusion_marks`、`commit_keeps_private_mark_of_opened_text`。`dyn.ps1` で、書き込み後も3つの印が残ることを確認 |
| S2 | 解消 | `.cargo/config.toml` で CRT を静的リンクした。exe は VCRUNTIME140.dll を読まなくなった | `tools/check-release.ps1`。偽の DLL を置いても起動し、エラーダイアログも出ない |
| S3 | 解消 | 隠すとき（Esc、Ctrl+Enter、× ボタン）に、編集欄の内容と「元に戻す」の履歴を消す | `hide_forgets_text_and_undo_history`、`close_button_forgets_text`。`residue.ps1` で、閉じた後の内容が空になることを確認 |
| S4 | 解消 | 64KB を超える config.toml は読まず、既定値と警告にする | `oversized_file_gives_default_with_warning`、`file_at_size_limit_is_read` |
| S5 | 解消（配布用ビルド） | `tools/build-release.ps1` が `--remap-path-prefix` でユーザーフォルダを `~` に置き換える。安定版の Cargo 1.99 では `trim-paths` が使えないため、普通の `cargo build --release` ではパスが残る | `check-release.ps1` |
| S6 | 解消 | `-C control-flow-guard` を指定した | `check-release.ps1` |
| S7 | 解消 | `is_enabled` が、登録されたパスと今の exe を比べる。古い登録は「無効」と表示し、選び直すと今のパスで登録し直す | `registered_only_when_value_points_to_this_exe`、`stale_registration_is_not_enabled` |
| S8 | 対応せず | 起動を妨げるだけで、それができるのは同じユーザーのアプリに限られる。名前を推測しにくくしても、同じユーザーなら見つけられるので効果がない | — |

配布用の exe は `pwsh -NoProfile -File tools/build-release.ps1` で作る。ビルドの最後に `check-release.ps1` が自動で実行される。

## 検査に使ったもの
- 一時ファイルの置き場所: 作業用の一時フォルダの `audit\`（リポジトリには入れていない）
- 内訳:
  - `dyn.ps1`: S1・S2・S4・細工メッセージの確認
  - `residue.ps1`: S3 の確認
  - `fuzzprobe\`: proptest 一式
- ツール: cargo-audit 0.22.2、cargo-deny 0.20.2、dumpbin 14.51、rustc 1.99.0
