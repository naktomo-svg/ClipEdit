# ClipEdit 設計書

初版: 2026-10-15（実装前の設計）
更新: 2026-10-16（セキュリティ監査 `docs/security-audit-2026-10-15.md` と負債の対応 `docs/tech-debt.md` の D1〜D30 を反映し、コードに合わせた）

コードを変えて、ここに書いた動き・関数の形・ファイル構成が変わったら、この設計書も一緒に直す。食い違ったときはコードが正しい。

## 目的

Windows でコピーしたテキストを、ショートカットキー一発で表示・編集し、編集結果をクリップボードに戻すユーティリティを Rust で作る。戻したあとの貼り付けは、利用者が自分で Ctrl+V する。

## 要件

| # | 要件 |
|---|---|
| R1 | 常駐型。タスクトレイにアイコンを置いて待機する |
| R2 | グローバルホットキー（既定 Ctrl+Alt+V）で編集ウィンドウを開く |
| R3 | 開いた時点のクリップボードのテキストを表示し、全文選択した状態にする。開いている間にもう一度ホットキーを押しても、編集中の内容は消さずに前面に出すだけ |
| R4 | 日本語 IME で問題なく編集できる。変換中の Enter / Esc は IME に任せ、ウィンドウを閉じない |
| R5 | Ctrl+Enter でクリップボードを編集後の内容に書き換えてウィンドウを隠す |
| R6 | Esc でクリップボードを変更せずにウィンドウを隠す（× ボタンも同じ動作） |
| R7 | クリップボードにテキストがない場合は空欄で開き、タイトルに「(テキストなし)」と表示する |
| R8 | トレイメニュー: 「開く」「ログイン時に起動」(チェック切替)「設定を読み直す」「終了」。アイコンの左クリックでも開く |
| R9 | ホットキーは `config.toml` で変更でき、起動し直さずに「設定を読み直す」で反映できる |
| R10 | 二重起動を防ぐ。2つ目に起動されたら、動いているほうの編集ウィンドウを開く |
| R11 | パスワード管理ソフトが付ける「履歴・クラウドに残さない」印を、書き戻しても消さない（監査 S1） |
| R12 | 隠したあとの編集欄に、テキストと「元に戻す」の履歴を残さない（監査 S3） |
| R13 | 表示倍率の違うモニターでも、ウィンドウ・文字・アイコンを倍率に合った大きさで出す |

対象外: 画像やファイルなどテキスト以外の編集、クリップボード履歴、元アプリへの自動貼り付け。

## 技術方針

- 言語: Rust（edition 2024、stable の cargo 1.99 で確認）
- GUI: Win32 API を直接使う（`windows` クレート 0.62）。編集欄は標準の複数行 EDIT コントロール。IME は OS 標準のまま動く
- 依存クレート: `windows`、`serde` と `toml`（設定ファイルの読み込み用）だけ。`deny.toml` で使ってよいライセンスを決めてある
- `#![windows_subsystem = "windows"]` でコンソールを出さない
- DPI: 起動直後に `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` を呼ぶ
- exe の作り（`build.rs` と `.cargo/config.toml`）:
  - マニフェスト `assets/clipedit.manifest` を埋め込む（Common Controls v6、`asInvoker`、Windows 10 / 11）
  - `assets/clipedit.ico` を exe のアイコンとして埋め込む。`build.rs` が `.res` を自分で作ってリンカーに渡すので、`rc.exe` も追加のクレートも使わない
  - C ランタイムを静的リンクする（監査 S2。exe と同じフォルダの `vcruntime140.dll` を読まない）
  - Control Flow Guard を有効にする（監査 S6）
  - 配布用は `tools/build-release.ps1` で作り、`--remap-path-prefix` で exe からユーザー名入りのパスを消す（監査 S5）

## 構成

```
ClipEdit/
├─ Cargo.toml / Cargo.lock / build.rs / deny.toml / config.example.toml
├─ .cargo/config.toml   静的リンクと CFG
├─ assets/              clipedit.ico（tools/make-icon.ps1 で生成）、clipedit.manifest
├─ src/
│  ├─ main.rs       起動、二重起動の判定、表示しない本体ウィンドウ、設定の適用と読み直し、通知、メッセージループ
│  ├─ instance.rs   2つ目に起動されたとき、動いているほうに「開いて」の合図を送る
│  ├─ editor.rs     編集ウィンドウ（EDIT コントロール、等幅フォント、キー処理、DPI への追従）
│  ├─ clipboard.rs  クリップボードのテキスト（CF_UNICODETEXT）と「残さない」印の読み書き
│  ├─ hotkey.rs     ホットキー文字列の解釈と RegisterHotKey / UnregisterHotKey、読み直しでのキーの切り替え
│  ├─ config.rs     config.toml の読み込みと既定値
│  ├─ tray.rs       トレイアイコン、ツールチップ、通知バルーン、右クリックメニュー、埋め込みアイコンの読み込み
│  ├─ autostart.rs  HKCU\Software\Microsoft\Windows\CurrentVersion\Run への登録と解除
│  └─ wide.rs       Win32 に渡す 0 終端の UTF-16 と文字列の変換
├─ tools/           配布用ビルド・exe の検査・zip 作成・まとめて検査・負債の兆候・アイコン生成
└─ docs/            この設計書・実装計画・監査報告・負債の台帳・手で確かめる手順・配布用 README
```

各モジュールの役割と公開する関数:

- `clipboard`
  - `ClipText { text: String, private: bool }`。`private` は「残さない」印が付いていたか
  - `read_text() -> Result<Option<ClipText>, String>`。テキストが無ければ `Ok(None)`
  - `write_text(owner: Option<HWND>, text: &str, private: bool) -> Result<(), String>`
  - `OpenClipboard` に失敗したら 50ms 間隔で最大 5 回まで再試行する
  - 「残さない」印は `ExcludeClipboardContentFromMonitorProcessing`（あれば印あり）、`CanIncludeInClipboardHistory` と `CanUploadToCloudClipboard`（DWORD の 0 なら印あり）。`private` のときは、3つの形式にそれぞれ DWORD の 0 を置き直す（除外の形式は、あること自体が印）
  - 書き込むものはすべて先にメモリを確保してから `EmptyClipboard` する。確保に失敗しても元の内容は消えない（台帳 D4）。印は本文より先に置くので、途中で失敗しても印の欠けた本文だけが残ることはない（台帳 D17）
- `hotkey`
  - `HotKey { mods: u32, vk: u32 }`
  - `parse("Ctrl+Alt+V") -> Result<HotKey, String>`（純粋関数）。修飾キーは `Ctrl`（`Control`）/ `Alt` / `Shift` / `Win` を1つ以上。キーは A〜Z・0〜9・F1〜F24・`Space`・`Insert`。大文字小文字は区別しない
  - `HotKey` の `Display`：`Ctrl+Alt+Shift+Win+キー` の決まった順と書き方。ツールチップと通知は、設定の書き方ではなくこれを出す（台帳 D42）
  - `register(hwnd, id, HotKey)`（`MOD_NOREPEAT` を付ける）、`unregister(hwnd, id)`
  - `switch(current, key, text, has_warning, register, unregister) -> (Option<Active>, Outcome)`（純粋関数。登録と解除は引数の関数で行う）。新しいキーを今のキーと別の ID（1 と 2）で登録できたときだけ今のキーを外す。設定に誤りがあるとき・同じキーのときは登録し直さない（台帳 D36）。`Active { id, key, text }` は使っているホットキー、`Outcome` は `Registered` / `Kept` / `Failed { wanted, kept }`
- `config`
  - `Config { hotkey: String }`、`DEFAULT_HOTKEY = "Ctrl+Alt+V"`
  - `default_path()`: exe と同じフォルダの `config.toml`
  - `load(path) -> (Config, Option<String>)`: ファイルが無ければ既定値で警告なし。64KB を超える・UTF-8 でない・TOML として壊れている場合は、既定値と警告文を返す（監査 S4）
- `editor`
  - `Editor::create(hinstance)`: 編集ウィンドウを隠したまま作る。文字数の上限は外す（既定の 32,767 文字では足りない）。編集欄は2つ作る（折り返す `edit` と、横スクロールの `edit_nowrap`）。`show_with` は、`WRAP_LIMIT`（5,000 文字）より長い行があれば `edit_nowrap` だけを表示してタイトルで知らせ、そうでなければ `edit` を表示する。折り返す編集欄は改行のない長い行で極端に遅くなるため（監査 S10。5 万文字で 11 秒）。閉じるときは両方を空にする
  - `open(read)`: 開いていれば前面に出すだけ。閉じていれば `read` でクリップボードを読んで `show_with` する
  - `show_with(Result<Option<ClipText>, String>)`: タイトルと本文を入れて全文選択し、マウスのあるモニターに出す
  - `hide()`: 隠し、編集欄のテキストと「元に戻す」の履歴を消す
  - `handle_key(&MSG) -> bool`: メッセージループから呼ぶキー処理
  - 確定時は editor の中で直接 `clipboard::write_text` を呼ぶ（本体への通知は不要）
- `tray`
  - `add(hwnd, tip) -> Result<(), String>`、`remove(hwnd)`、`set_tip(hwnd, tip)`、`notify(hwnd, kind, title, message) -> bool`（`kind` は `NoticeKind::Info` か `Warning`。うまくいった知らせだけ Info。台帳 D34）、`show_menu(hwnd, autostart_on)`
  - `load_app_icon(size)`: exe に埋め込んだ `assets/clipedit.ico` から、指定の大きさに合う画像を選んでアイコンを作る。トレイと編集ウィンドウで使う
  - `PendingNotices`: 出せなかった通知を覚えておき、トレイアイコンを追加できたときに送り直す（台帳 D3）
- `autostart`
  - `is_enabled()`: Run キーの値が、今動いている exe を指しているときだけ true。exe を移動したあとの古い登録は無効と扱う（監査 S7）
  - `set(enabled)`: 値は exe のフルパスを引用符で囲んで登録する
- `instance`
  - `forward_to_running(class) -> bool`: 動いている本体ウィンドウを探し、前面に出す権利を譲ってから `WM_OPEN_REQUEST` を送る。1つ目が本体ウィンドウを作り終える前に起動されたときのため、見つからなければ 100ms ごとに約 2 秒まで探し直す（台帳 D37）

## 処理の流れ

1. 起動: Mutex `Local\ClipEdit-SingleInstance` を作る。既にあれば `instance::forward_to_running` で動いているほうに編集ウィンドウを開かせ、自分は終了する。届かなければ「応答しません」とメッセージボックスで知らせてから終了する（台帳 D41）
2. DPI の設定をし、編集ウィンドウを隠したまま作る。次に、表示しない本体ウィンドウを作る（`TaskbarCreated` を受け取るため `HWND_MESSAGE` にはしない）。どちらかの作成に失敗したら、理由をメッセージボックスで出して終了する（台帳 D2）
3. `config.toml` を読んでホットキーを登録し、トレイアイコンを追加する。ツールチップは「ClipEdit (Ctrl+Alt+V)」、登録できなければ「ClipEdit (ホットキーなし)」。設定に問題があった・登録できなかったときは通知で知らせる
4. `WM_HOTKEY`・トレイの左クリック・「開く」・2つ目の起動からの合図: `editor.open(clipboard::read_text)`
5. 表示: マウスカーソルのあるモニターの作業領域の中央に、800×500 を表示倍率に合わせて広げた大きさで出す（作業領域より大きければ縮める）。文字は「BIZ UDゴシック」11pt。`SetForegroundWindow` で前面に出し、最小化されていたら元に戻す
6. 編集欄で Ctrl+Enter: `write_text` → 成功したら隠す。失敗したらタイトルにエラーを出し、ウィンドウは開いたままにする
7. Esc または ×: 隠す（ウィンドウは破棄しない）
8. 「設定を読み直す」: `config.toml` を読み直し、新しいホットキーを今のものと別の ID（1 と 2 を交互に使う）で登録できたときだけ今のものを外す。設定に誤りがあるとき・新しいキーを登録できないときは、今のホットキーを使い続ける（台帳 D36）。ツールチップを更新し、結果を通知する
9. エクスプローラーが再起動したら（`TaskbarCreated`）、トレイアイコンを追加し直す
10. 「終了」: 本体ウィンドウを壊し、ホットキーを解除し、トレイアイコンを消して `PostQuitMessage`

キー処理は、メッセージループで `TranslateMessage` の前に、編集欄宛ての `WM_KEYDOWN` を見て行う。

| キー | 動作 |
|---|---|
| Ctrl+Enter | クリップボードに書き込んで隠す |
| Esc | 隠す |
| Ctrl+A | 全文選択 |
| Ctrl+Backspace | 直前の1語を消す。語の切れ目は、空白・英数字・ひらがな・カタカナ・漢字の切り替わりと改行。標準の EDIT は □（0x7F）を入れてしまうので自前で処理する |

IME で変換中のキーは `VK_PROCESSKEY` として届くので、上のどれにも当たらず IME が処理する。

改行コードは、表示するときに CRLF にそろえ（EDIT コントロールの都合）、書き戻すときは CRLF のままにする。

DPI: `WM_DPICHANGED` を受けたら、システムが勧める位置と大きさに移し、フォントとアイコン（タイトルバー用の小、タスクバー用の大）を新しい倍率で作り直す。ただし `show_with` が位置を決めている最中は、勧められた位置を使わない。

## エラー処理

| 状況 | 対応 |
|---|---|
| 起動時にウィンドウを作れない | 理由をメッセージボックスで出して終了する |
| ホットキー登録の失敗（他のアプリが使用中） | 起動時は「ホットキー X を登録できませんでした。トレイアイコンから開けます。」と通知し、ホットキーなしで常駐を続ける。既定のキーには切り替えない。読み直しでは「ホットキー X を登録できませんでした。Y のまま使えます。」と通知し、今のキーを使い続ける（台帳 D36） |
| config.toml が大きすぎる・UTF-8 でない・書式が不正、またはホットキー文字列が不正 | 既定値で起動し、「ClipEdit: 設定を既定値で起動しました」と理由を通知する |
| トレイの準備ができる前で通知を出せない | 覚えておき、トレイアイコンを追加できたときに出す |
| クリップボードを開けない（再試行後も失敗） | 読み込み時: 空欄で開き、タイトルに理由を出す。書き込み時: 手順 6 のとおり |
| テキスト以外がコピーされている | R7 のとおり |
| 自動起動のレジストリ操作の失敗 | 通知で知らせ、チェック状態は変えない |

## テスト

- 普通のテスト（`cargo test`）: 純粋な関数
  - `hotkey::parse`、`config` の読み込み（ファイルなし・正しい・壊れている・大きすぎる・上限ちょうど）、ホットキーの代わりの決定とツールチップ・通知の文
  - 改行の統一、タイトルの文、ウィンドウの位置（`center_rect`）、Ctrl+Backspace の語の切れ目
  - アイコンの画像の選び方、通知の送り直し、「残さない」印の判定、Run キーの値と exe の照合
- 実機のテスト（`#[ignore = "理由"]` 付き。`cargo test -- --ignored --test-threads=1`、または `tools/check-all.ps1 -Hardware`）: クリップボード・ウィンドウ・レジストリを実際に使う
  - クリップボードの往復と「残さない」印、テキスト以外のとき
  - 編集ウィンドウ（長い本文、開き直しで消えない、Ctrl+Backspace、隠したあとの残留、最小化からの復帰、アイコンの大きさ）
  - 自動起動の登録と、古い登録を無効と見ること（失敗しても元の値に戻す）
  - 2つ目の起動の合図、アイコンの読み込み
- exe の検査（`tools/check-release.ps1`。`build-release.ps1` が最後に流す）: 静的リンク、CFG、ASLR、DEP、ユーザー名入りのパスがないこと、マニフェストとアイコンが入っていること
- 手で確かめる項目（自動にできないもの）: `docs/manual-checks.md`（表示倍率の違うモニター、IME、ツールチップと通知、エクスプローラーの再起動、高い表示倍率）
- まとめて流す: `tools/check-all.ps1`（書式・clippy・テスト・負債の兆候）
