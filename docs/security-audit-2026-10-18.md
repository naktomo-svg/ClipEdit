# ClipEdit セキュリティ監査報告（2回目、2026-10-18）

対象: master `5f47da4`、`target\release\clipedit.exe`（`c2ecce3` でビルド。そのあとの変更は文書だけ）
計画: `docs/superpowers/plans/2026-10-18-security-audit.md`
前提: ネットワーク通信なし。ログイン中のユーザーの権限（asInvoker、中の整合性レベル）で常駐する。**配布する前提で評価した。** 同じユーザー・同じ整合性レベルのアプリは、もともとクリップボードもプロセスのメモリも読めるので、それによる指摘は「情報」とした。

## 指摘一覧

| ID | 重要度（配布する場合） | 自分専用なら | 内容 | 確認方法 |
|---|---|---|---|---|
| S9 | **高** | 中 | exe と同じフォルダに置いた DLL を5つ読み込む（`WindowsCodecs.dll`・`wtdccm.dll`・`CRYPTBASE.DLL`・`profapi.dll`・`XmlLite.dll`） | 実機で再現 |
| S10 | 低 | 低 | 改行のない長い行を開くと固まる（52 万文字で 41 秒、524 万文字以上は 2 分たっても開かない）。クリップボードには、整合性レベルの低いプロセスからも書き込める | 実機で計測 |
| S11 | 情報 | 情報 | 整合性レベルの低いプロセスも、`WM_GETTEXT` で開いている編集欄の中身を読める | 実機で再現 |
| S12 | 情報 | 情報 | 同じユーザーのプロセスは、メッセージで編集ウィンドウを開く・自動起動を切り替える・終了させる・編集欄を書き換える、ができる | 実機で再現 |
| S13 | 情報 | 情報 | 2回目の起動は、クラス名 `ClipEditMain` で見つけた窓が本物かを確かめずに、前面に出る権利と合図を渡す | コードを読む（実機の試験用の偽物は Defender に止められた） |
| S14 | 情報 | 情報 | 閉じたあとも、パスワードの写しがプロセスのメモリ（ヒープ）に残る | メモリダンプ |
| S15 | 情報 | 情報 | `apply_font` / `apply_icons` の SAFETY コメントの前提（前のフォント・アイコンは自分で作ったもの）が、ほかのプロセスから `WM_SETFONT` / `WM_SETICON` を送られると崩れる | コードを読む |
| S16 | 情報 | 情報 | `center_rect` に極端な座標を渡すと、デバッグビルドで整数があふれてパニックする | ランダム入力 |
| S17 | 方針 | — | exe と zip に署名がない。初めて起動するときに SmartScreen の警告が出て、利用者には本物かを確かめる手段がない | 調査（下の「署名と配布の完全性」） |

### S9 exe のフォルダの DLL を読み込む（高）
- **内容**: ClipEdit が直接読み込む DLL は、KnownDLLs と API セットだけ（`dumpbin /dependents`）。ところが、動いている途中でシステムの DLL が名前だけで読み込む DLL があり、Windows はそれを、まず exe と同じフォルダから探す。前回の S2（`vcruntime140.dll`）は静的リンクで消えたが、同じ種類の経路がほかにも残っていた。
- **再現**: ClipEdit を動かして、編集ウィンドウを開き、設定の読み直しで通知も出す。そのとき読み込んだ 90 個の DLL のうち、KnownDLLs でない System32 の DLL は 63 個あった。その本物の写しを exe のフォルダに置いて同じ操作をすると、5つが exe のフォルダから読み込まれた（`b6.ps1`）。
  - 読み込まれたもの: `WindowsCodecs.dll`・`wtdccm.dll`・`CRYPTBASE.DLL`・`profapi.dll`・`XmlLite.dll`
- **影響**: 「ダウンロード」フォルダなど、ほかのアプリやブラウザが書き込めるフォルダに exe を置くと、そこに置かれた DLL が ClipEdit の権限で動く。自動起動を登録していれば、ログインのたびに動く。配布用 README は「ダウンロード」フォルダに置かないよう書いているが、守られない前提で考える。
- **対策**: 次の2つを行い、`b6.ps1` で0個になることを確かめる。
  1. `main` の最初（ウィンドウを作る前）で `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` を呼ぶ。こうすると、そのあとの名前だけの `LoadLibrary` は System32 だけを探す
  2. リンカーに `/DEPENDENTLOADFLAG:0x800`（`LOAD_LIBRARY_SEARCH_SYSTEM32`）を渡す。起動時に読み込む DLL にも同じ制限をかける
  - あわせて `check-release.ps1` に、`dumpbin /loadconfig` で「Dependent Load Flag」が 0x800 であることの検査を足す

### S10 改行のない長い行で固まる（低）
- **内容**: 複数行の EDIT コントロールは、ウィンドウの幅で折り返して表示する。改行も空白もない長い行は、折り返しの計算がとても重い。そのうえ、クリップボードのテキストの大きさに上限がない。
- **計測**（`b4.ps1`。「開いた」は、編集ウィンドウが表示されて本体ウィンドウが応答を返すまで）:

  | 内容 | 文字数 | 開くまで | 使ったメモリ（ピーク） |
  |---|---|---|---|
  | 改行なしの漢字の連続（100KB） | 5 万 | 3.7 秒 | 32MB |
  | 改行なしの漢字の連続（1MB） | 52 万 | 41 秒 | 19MB（ほかより小さく、測り方に問題があった可能性がある） |
  | 改行なしの漢字の連続（10MB / 50MB） | 524 万 / 2,621 万 | 2 分たっても開かない | 81MB / 361MB |
  | 1 行 80 文字の英文（1MB / 10MB） | 52 万 / 524 万 | 0.4 秒 / 3.2 秒 | 34MB / 54MB |

- **影響**: 開いている間、ClipEdit は固まり、ホットキーもトレイも効かない。圧縮した JSON や Base64 のように、改行のない長いテキストは実際によくある。整合性レベルの低いプロセスもクリップボードに書き込めるので、ほかのアプリが ClipEdit を止めることもできる。ただし、止めるだけで情報は漏れない。また、利用者がホットキーを押したときにだけ起きる。
- **対策の候補**: どれを選ぶかは、使い勝手と合わせて決める。
  1. 開く前に文字数を見て、一定（たとえば 100 万文字）を超えたら開かずにタイトルで知らせる
  2. 折り返しをやめる（`ES_AUTOHSCROLL` と横スクロールバー）。どれだけ速くなるかは、試してから決める
  3. 大きいものを読むときの上限を決め、メモリの使い過ぎも防ぐ（unsafe レビューの指摘 F1）

### S11 低い整合性レベルからも編集欄を読める（情報）
- **内容**: UIPI（整合性レベルが低いプロセスからのメッセージを止める仕組み）は、`WM_GETTEXT` を止めない。
- **再現**: 低い整合性レベル（S-1-16-4096）の PowerShell から、ClipEdit の編集欄に `WM_GETTEXT` を送ると、中身が返った。同じプロセスからの `WM_SETTEXT`・`WM_COMMAND`・`WM_OPEN_REQUEST`・`WM_DPICHANGED` は、どれもアクセス拒否（5）で届かなかった（`b1b2.ps1`）。
- **影響が小さい理由**: 同じ低い整合性レベルのプロセスは、クリップボードを直接読み書きできた（`Clipboard.GetText` / `SetText` が通った）。そのため、新しく漏れるのは「編集したが、まだ書き戻していない中身」だけ。
- **対策の候補**: 編集欄をサブクラス化して、ほかのスレッドから送られた `WM_GETTEXT` / `WM_GETTEXTLENGTH` を断る（`InSendMessageEx` で見分ける）。ただしスクリーンリーダーも同じ方法で読むので、「残さない」印の付いたテキストのときだけにするなどの判断が要る。今は対策しなくてよいと考える。

### S12 同じユーザーのプロセスからのメッセージ（情報）
同じユーザー・同じ整合性レベルの PowerShell から送ると、次の操作がすべて効いた（`b1b2.ps1`）。
- `WM_OPEN_REQUEST` で編集ウィンドウを開く
- `WM_COMMAND` で自動起動を切り替える（Run キーの値が変わった。試験のあとで元に戻した）
- `WM_COMMAND` で終了させる
- `WM_GETTEXT` / `WM_SETTEXT` で編集欄を読む・書き換える

このプロセスは、Run キーもクリップボードも直接扱えるので、メッセージでしかできないことはない。本体ウィンドウを `HWND_MESSAGE` にすれば一部は届かなくなるが、`TaskbarCreated` を受け取れなくなる（エクスプローラーの再起動でトレイアイコンが消える）。そのため、今の作りのままでよいと判断した。

### S13 偽の本体ウィンドウ（情報）
`instance::forward_with_retries` は、`FindWindowW("ClipEditMain")` で最初に見つかった窓のプロセスに、`AllowSetForegroundWindow` と `WM_OPEN_REQUEST` を渡す。同じユーザーのプロセスが同じクラス名の窓を先に作っておくと、2回目の起動はそちらに前面に出る権利を渡し、「届いた」として何も表示せずに終わる。前回の S8（単一起動の印の横取り）と同じく、同じユーザーにしかできないので対策は不要と判断した。実機の試験用に作った偽の窓のプログラムは、Windows Defender に「望ましくない可能性のあるソフトウェア」として止められたので、コードを読んで確かめた結果だけを書いている。

### S14 メモリに残る写し（情報）
試験用のパスワードを開いてから閉じ、そのあとクリップボードを別の内容に替えて、プロセスのメモリダンプで探した（`b7.ps1`）。

| 場面 | 見つかった数 |
|---|---|
| 開いている間 | 4 |
| Esc で閉じたあと | 3 |
| Ctrl+Enter で書き込んで閉じたあと | 6 |

編集欄の中身と「元に戻す」の履歴は消えている（S3 の対応は効いている）。残っているのは、Rust の `String` / `Vec<u16>` や、Windows が中で使った領域の解放済みのメモリ。同じユーザーはもともとメモリを読めるので、情報に留める。減らしたいなら、読み込みと書き込みで使うバッファを使い終わったときに 0 で埋める。ただし、EDIT コントロールや Windows の中の写しまでは消せない。

### S15 SAFETY コメントの前提（情報）
`apply_font` は「前のフォントは自分で作ったもの」、`apply_icons` は「前のアイコンは自分で付けたもの」として、`WM_SETFONT` / `WM_SETICON` が返したハンドルを破棄している。ほかのプロセスが自分のハンドルを送り込んでいると、次に表示倍率が変わったときにそれを破棄しようとする。ただし、ほかのプロセスのハンドルなので失敗するだけで、自分のフォントかアイコンが1つ漏れる程度。SAFETY コメントを実際に合わせて直す。`wnd_proc` の `WM_DPICHANGED` の SAFETY コメントには、ほかのプロセスから送られても安全な理由（`PostMessage` は 1159 で断られ、`SendMessage` でもでたらめなポインタは届かなかった）を書き足す。

### S16 `center_rect` の整数あふれ（情報）
ランダム入力で、`left = 1167600539, right = -979883110` のような作業領域を渡すと、デバッグビルドで `work.right - work.left` があふれてパニックした。リリースビルドでは値が回り込むだけ。入力は `GetMonitorInfoW` の作業領域なので、現実には起きない。直すなら `saturating_sub` を使う。

## 署名と配布の完全性（S17）

日本の個人が、無料の MIT のツールを zip で配る場合の選択肢を、2026-10-18 時点の一次情報で比べた（未確認の点は「未確認」と書いた）。

| 選択肢 | 日本の個人が使えるか | 費用（年） | SmartScreen の警告 | GitHub Actions から | 手間 |
|---|---|---|---|---|---|
| 署名なし＋SHA256＋GitHub の artifact attestation | 使える | 無料 | 消えない（評判はファイルごと） | `actions/attest-build-provenance` | 低 |
| winget（コミュニティのリポジトリ） | 使える | 無料 | 消えない（ウイルス対策のスキャンと審査を通るので、配布元の確認にはなる） | マニフェストの PR を自動化できる | 低〜中 |
| Microsoft Store（MSIX） | 使える（個人の登録は 2025 年 9 月から無料） | 無料 | Store から入れれば出ない（MS が署名する） | MSIX のビルドは可能 | 中〜高（自動起動を `StartupTask` に移す） |
| SignPath Foundation（OSS 向け） | 条件付き（公開リポジトリ・CI でのビルド・手動承認など。評判のない新しいプロジェクトは断られることがある） | 無料 | 署名を続けると評判が育つ。発行元の名前は「SignPath Foundation」になる | 前提が CI | 中（審査あり） |
| 個人向け（IV/OV）の証明書（例: SSL.com IV＋eSigner） | 使える（SSL.com への日本からの申込は未確認） | 約 $130〜310 | 署名を続けると評判が育つ（すぐには消えない） | eSigner の公式の手順あり | 中（本人確認、毎年の更新） |
| Certum のオープンソース開発者向け | 使える（個人限定） | €49〜 | 同上 | スマホのワンタイムコードが要り、自動化しにくい | 中（調べた時点で在庫切れの表示） |
| EV 証明書 | 原則は法人向け | $400〜 | 2024 年からは OV と同じ扱い（MS は SmartScreen のためには勧めていない） | — | 高 |
| Azure Artifact Signing（旧 Trusted Signing） | **使えない**（個人は米国とカナダだけ） | — | — | — | — |

補足:
- どの証明書でも、署名するときに RFC 3161 のタイムスタンプを付ける（`signtool /tr <TSA> /td sha256`）。2026 年 3 月 1 日以降に発行される証明書の有効期間は最大 460 日だが、タイムスタンプがあれば、証明書が切れたあとも署名は有効なまま。
- attestation で分かるのは「この zip がこのリポジトリのこのワークフローで作られた」ことで、安全だという保証ではない。利用者が確かめなければ意味がない。また SHA256 は、配布ページそのものを書き換えられると役に立たない。

**推奨**（判断は利用者）:
1. **まず**: タグを push したら zip・SHA256SUMS・artifact attestation を作って GitHub Release に上げるワークフローを作る。配布用 README に確かめ方を書く。無料で、ほかのどの選択肢にも共通の土台になる。
2. **続けて**: winget に登録する。
3. **SmartScreen の警告をなくしたいなら**: 無料でできるのは Microsoft Store（MSIX）だけ。ただし自動起動を `StartupTask` に移し、パッケージ化する作業が要る（MSIX から HKCU の Run に書いた場合の動き、グローバルホットキーが Store の規約上問題ないかは未確認）。
4. **zip のまま署名したいなら**: SignPath Foundation に申請し、断られたら CI から署名できる個人向けの証明書にする。

出典: Microsoft Learn「Code signing options」（2026-08-29）、「SmartScreen reputation」、Artifact Signing Quickstart、MSI/EXE の要件（2026-08-24）、Windows Developer Blog「Free developer registration for individual developers on Microsoft Store」（2025-09-10）、StartupTask のリファレンス（2026-06-17）、winget の提出と検証（2026-07-14）、Certum・SSL.com の商品ページ、SignPath Foundation の規約、CA/B Forum の CSC-31 と Code Signing BR v3.11.0（2026-06-16）、GitHub Docs「Artifact attestations」（いずれも 2026-10-18 に参照）。

## 問題なしだった項目
- **unsafe の全件レビュー（72 ブロック）**:
  - 範囲外の読み書き・二重解放・解放後の使用・スレッドをまたぐ使用は見つからなかった。前回のあとで増えた 26 ブロックも含む。
  - `read_text` は `GlobalSize` までしか読まない。`read_dword` は 4 バイト以上あるか確かめてから読む。
  - `write_text` は、`SetClipboardData` に失敗した分から先だけを解放する。
  - `RegGetValueW` の2回呼びは、間に値が大きくなっても `ERROR_MORE_DATA` で None を返すだけ。
  - `pick_icon_image` は `data.get` で範囲を確かめ、u32 を usize にしてから足すのであふれない。
  - SAFETY コメントの食い違いは S15 の2か所だけ。
- **外からのデータでのパニック**: クリップボード・config.toml・ウィンドウメッセージ・レジストリから届く `expect` / `unwrap` / 添字はなかった。`expect` は既定のホットキー（定数）の解析だけ。
- **`WM_DPICHANGED` のポインタ**: ほかのプロセスから `PostMessage` すると、Windows が `ERROR_MESSAGE_SYNC_ONLY`（1159）で断る。でたらめなポインタ（0x10）付きの `SendMessage` も届かず、ClipEdit は動き続けた。
- **整合性レベルの低いプロセス**: `WM_GETTEXT` 以外（開く・自動起動の切り替え・終了・`WM_SETTEXT`・`WM_DPICHANGED`）は、UIPI がアクセス拒否で止めた。
- **細工したクリップボード**: 終端の 0 がない、0 バイト、奇数バイト、片方だけのサロゲート、2 バイトしかない `CanIncludeInClipboardHistory`。どれも落ちずに開いた。
- **ランダム入力（proptest、19 種類、リリースで各 20 万件・デバッグで各 2 万〜5 万件）**:
  - 対象: ホットキーの解析と表示の往復、`hotkey::switch` の切り替え（1 件で最大 40 回）、config の解析（20 万段の入れ子、100 万文字の行、BOM、NUL、CR だけの改行）、`copy_wide`、`wide`、`prev_word_start`、改行の統一、`pick_icon_image`。
  - パニックは S16 だけで、性質もすべて成り立った。
  - 深い入れ子は、本番と同じ 1MiB のスタックで試した。toml の再帰の上限でエラーになり、スタックはあふれなかった。
  - `switch` は、OS に登録されている ID と、今のホットキーの ID がいつも一致した。
- **前回の指摘（S1〜S7）の回帰**:
  - 実機のテスト 18 件がすべて通った。S1 の印、S3 の閉じたあとの中身、S7 の古いパスの登録も、このテストで確かめている。
  - `check-release.ps1` も通った。S2 の静的リンク、S5 のユーザー名のパス、S6 の CFG はこちらで確かめている。
  - S4 の 64KB の上限は、ふつうのテストで確かめている。
- **exe の設定**: 高エントロピー ASLR・DEP・CFG・/GS が有効。マニフェストは `asInvoker`（`uiAccess="false"`）。ユーザー名のパスは入っていない。
- **依存ライブラリ**: `cargo audit` で 29 crate に既知の脆弱性はなかった（勧告 1,338 件）。`cargo deny` は advisories / bans / licenses / sources がすべて ok。
- **画面に出す文字列**: 通知・タイトル・メッセージボックスに、クリップボードの中身は出ない。config.toml について出すのは、行番号と toml の短い説明だけ。
- **CI（`.github/workflows/ci.yml`）**:
  - トリガーは `pull_request`（`pull_request_target` ではない）なので、フォークからのプルリクエストは読み取りだけのトークンで動き、秘密の情報には触れない。
  - `permissions: contents: read`、`persist-credentials: false`。外部の Action はすべてコミットのハッシュで固定。
  - 書き込みの経路はない。Action の版を上げる仕組み（Dependabot など）はないので、上げるときは手で行う。
- **配布用 zip**: 中身は `clipedit.exe`・`README.md`・`config.example.toml`・`LICENSE`・`THIRD-PARTY-NOTICES.txt` の5つだけ。

## 推奨する修正順
1. **S9**（配布の前に必ず直す）: `SetDefaultDllDirectories` と `/DEPENDENTLOADFLAG`。`b6.ps1` と同じ確認を `check-release.ps1` に組み込む
2. **S17 の土台**: タグから zip・SHA256SUMS・attestation を作る Release のワークフロー。署名をどうするかは、上の比較から利用者が決める
3. **S10**: 開く前の文字数の上限か、折り返しをやめるか。測ってから決める
4. **S15・S16**（小さな修正）: SAFETY コメントを直す、`saturating_sub` にする
5. S11〜S14 は対策しない（情報として残す）。S11 は、スクリーンリーダーへの影響を見てから判断する

## 対応結果（2026-10-18）

| ID | 状態 | 対応 | 確認 |
|---|---|---|---|
| S9 | 解消（97eedd9） | `main` の最初で `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` を呼び、`build.rs` でリンカーに `/DEPENDENTLOADFLAG:0x800` を渡す | `dll_next_to_exe_is_not_loaded`（直す前は失敗）。`check-release.ps1` が Dependent Load Flag を検査する。`b6.ps1` で、63 個の DLL を置いても exe のフォルダから読み込むものが0個になった |
| S17 | 土台を作った（a60d5ee） | `v*` のタグで zip・`SHA256SUMS.txt`・artifact attestation を GitHub Release に上げる `release.yml`。署名そのものは利用者が決める | YAML として読めること、版の確認、リリースノートの組み立て、照合の手順を手元で確かめた。リモートがないので GitHub の上ではまだ動かしていない |
| S10 | 解消（0193787） | 編集欄を2つにし、5,000 文字を超える行があるテキストは、折り返さない編集欄で開く（タイトルで知らせる） | 直す前に測った値：折り返さなければ 262 万文字の1行が 0.3 秒。`long_line_opens_quickly_without_wrapping`（20 万文字が 3 秒以内）ほか。上限の 5,000 文字は、画面がロックされて描画を含めた時間を測れなかったので安全側にした（手の確認 D8-4d で確かめる） |
| S15 | 解消（db6c9c0） | SAFETY コメントを実際に合わせた | — |
| S16 | 解消（db6c9c0） | `center_rect` を i64 で計算し、大きさを 0 以上にした | `extreme_work_area_does_not_overflow`（直す前は失敗） |
| S11〜S14 | 対応せず | 情報として残す | — |

あわせて、測り方の誤りを1つ見つけた。この報告の S10 の表（`b4.ps1`）は、開く合図の `PostMessage` のあとに `SendMessage` の応答を待って「開き終わった」としていたが、送ったメッセージが先に処理されることがあり、時間が短く出ることがある。直したときの計測（`measure.ps1`）は、編集欄に期待した文字数が入るまで `WM_GETTEXTLENGTH` で待つ方法に変えた。こちらの値は、折り返しありで改行なし 5 万文字が 11 秒、52 万文字は 90 秒たっても終わらず、報告の表より悪かった。
## 検査に使ったもの
- 一時ファイルの置き場所: 作業用の一時フォルダ（リポジトリには入れていない）
  - `audit\send.ps1`・`audit\native.cs`: ほかのプロセスからメッセージを送る（Windows PowerShell 5.1。低い整合性レベルのプロセスは一時フォルダに書けないので、P/Invoke は csc で作った DLL にした）
  - `audit\b1b2.ps1`: B1・B2（低い整合性レベル／同じユーザーから送る。PowerShell の複製に `icacls /setintegritylevel low` を付けて使い、終わったら消した）
  - `audit\b4.ps1`: 細工したクリップボードと大きいテキスト、`WM_DPICHANGED` の `SendMessage`
  - `audit\b6.ps1`: exe のフォルダの DLL を読み込むか（System32 の本物の写しを置いて試し、終わったら消した）
  - `audit\b7.ps1`: メモリダンプで、閉じたあとの写しを探す（ダンプは消した）
  - `audit\fakemain.cs`: S13 の偽の窓（ビルドした exe は Defender が隔離した）
  - `fuzzprobe\`: proptest 一式（`pick_icon_image` は private なので、`5f47da4` の本文を写して試した）
- ツール: cargo-audit 0.22.2、cargo-deny 0.20.2、dumpbin（Visual Studio）、csc（.NET Framework 4）、proptest 1.12、rustc 1.99.0
- 試験の影響: クリップボードの中身は何度も書き換えた。自動起動の Run キーの値は、試験の途中で変わったものを元に戻した（最後に元の値と同じであることを確かめた）。ClipEdit は止めてある。
