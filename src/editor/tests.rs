use super::*;
use crate::textops::WRAP_LIMIT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE;
use windows::Win32::UI::WindowsAndMessaging::{
    GWLP_USERDATA, HICON, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, SetMenu, SetWindowLongPtrW,
    WM_CHAR, WM_COPY, WM_CUT, WM_PASTE,
};

#[test]
fn title_reflects_content() {
    assert_eq!(title_for(&plain("x")), TITLE);
    assert!(title_for(&Ok(None)).ends_with("(テキストなし)"));
    assert!(title_for(&Err("開けません".into())).ends_with("開けません"));
}

fn plain(text: &str) -> Result<Option<ClipText>, String> {
    Ok(Some(ClipText {
        text: text.to_string(),
        private: false,
    }))
}

fn new_editor() -> Editor {
    // SAFETY: None は自分の exe を指し、返るハンドルは解放不要。
    let instance = unsafe { GetModuleHandleW(None) }.unwrap().into();
    Editor::create(instance).unwrap()
}

// 実際にウィンドウを出すので通常は実行しない: cargo test -- --ignored --test-threads=1
#[test]
#[ignore = "実際にウィンドウを出す"]
fn shows_text_longer_than_default_edit_limit() {
    let editor = new_editor();
    let long = "あ".repeat(100_000);
    editor.show_with(plain(&long));
    assert_eq!(editor.text(), long);
    editor.hide();
}

fn title_of(editor: &Editor) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW as get;
    let mut buf = [0u16; 256];
    // SAFETY: 自分のウィンドウのタイトルを、buf の長さまで受け取る。
    let len = usize::try_from(unsafe { get(editor.hwnd, &mut buf) }).unwrap_or_default();
    String::from_utf16_lossy(&buf[..len])
}

// 監査 S10: 改行のない長い行は、折り返さない編集欄ですぐに開く（折り返すと 5 万文字で 11 秒かかった）
#[test]
#[ignore = "実際にウィンドウを出す"]
fn long_line_opens_quickly_without_wrapping() {
    let editor = new_editor();
    let long = "x".repeat(200_000);
    let started = std::time::Instant::now();
    editor.show_with(plain(&long));
    let elapsed = started.elapsed();
    assert!(elapsed.as_secs() < 3, "開くのに {elapsed:?} かかった");
    assert_eq!(editor.active(), editor.edit_nowrap);
    assert_eq!(editor.text(), long);
    assert!(
        title_of(&editor).contains("折り返さず"),
        "{}",
        title_of(&editor)
    );
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn short_lines_use_the_wrapping_editor() {
    let editor = new_editor();
    editor.show_with(plain(&"x".repeat(WRAP_LIMIT + 1)));
    editor.hide();
    editor.show_with(plain("短い行\r\nもう1行"));
    assert_eq!(
        editor.active(),
        editor.edit,
        "前に長い行を開いていても、短ければ折り返す"
    );
    assert_eq!(editor.text(), "短い行\r\nもう1行");
    assert_eq!(title_of(&editor), TITLE);
    editor.hide();
}

// 監査 S3・S10: 閉じたら、どちらの編集欄にも中身を残さない
#[test]
#[ignore = "実際にウィンドウを出す"]
fn hide_forgets_text_in_both_editors() {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW as len_of;
    let editor = new_editor();
    editor.show_with(plain(&"s".repeat(WRAP_LIMIT + 1)));
    editor.hide();
    editor.show_with(plain("secret"));
    editor.hide();
    // SAFETY: 自分の編集欄に値を渡すだけ。
    let lens = unsafe { (len_of(editor.edit), len_of(editor.edit_nowrap)) };
    assert_eq!(lens, (0, 0));
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn open_while_visible_keeps_edits() {
    let editor = new_editor();
    editor.show_with(plain("元の内容"));
    // SAFETY: 文字列は w! で作った 0 終端の定数。
    unsafe {
        let _ = SetWindowTextW(editor.edit, w!("編集中"));
    }
    editor.open(|| panic!("表示中は読み直さない"));
    assert_eq!(editor.text(), "編集中");
    editor.hide();
    editor.open(|| plain("新しい内容"));
    assert_eq!(editor.text(), "新しい内容");
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn ctrl_backspace_deletes_previous_word_without_inserting_char() {
    let editor = new_editor();
    editor.show_with(plain("hello 日本語のテキスト"));
    // SAFETY: EM_SETSEL はポインタを渡さない。
    unsafe {
        SendMessageW(
            editor.edit,
            EM_SETSEL,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(-1)),
        );
    }
    assert!(editor.press(VK_BACK.0, true, false, false));
    assert_eq!(editor.text(), "hello 日本語の");
    assert!(editor.press(VK_BACK.0, true, false, false));
    assert_eq!(editor.text(), "hello 日本語");
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn ctrl_backspace_deletes_selection() {
    let editor = new_editor();
    editor.show_with(plain("abc def"));
    // SAFETY: EM_SETSEL はポインタを渡さない。
    unsafe {
        SendMessageW(editor.edit, EM_SETSEL, Some(WPARAM(1)), Some(LPARAM(3)));
    }
    assert!(editor.press(VK_BACK.0, true, false, false));
    assert_eq!(editor.text(), "a def");
    editor.hide();
}

fn can_undo(editor: &Editor) -> bool {
    use windows::Win32::UI::Controls::EM_CANUNDO;
    // SAFETY: EM_CANUNDO はポインタを渡さない。
    unsafe { SendMessageW(editor.edit, EM_CANUNDO, None, None) }.0 != 0
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn hide_forgets_text_and_undo_history() {
    let editor = new_editor();
    editor.show_with(plain("secret"));
    // SAFETY: EM_SETSEL はポインタを渡さない。EM_REPLACESEL の文字列は w! で作った 0 終端の定数。
    unsafe {
        SendMessageW(editor.edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        SendMessageW(
            editor.edit,
            EM_REPLACESEL,
            Some(WPARAM(1)),
            Some(LPARAM(w!("edited").as_ptr() as isize)),
        );
    }
    assert!(can_undo(&editor));
    editor.hide();
    assert_eq!(editor.text(), "");
    assert!(!can_undo(&editor), "元に戻すで secret が出てこない");
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn close_button_forgets_text() {
    use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
    let editor = new_editor();
    editor.show_with(plain("secret"));
    // SAFETY: WM_CLOSE はポインタを渡さない。
    unsafe {
        SendMessageW(editor.hwnd, WM_CLOSE, None, None);
    }
    assert!(!editor.is_visible());
    assert_eq!(editor.text(), "");
}

// ほかのプロセスにメニューを外されても、× で閉じると中身が消える（監査 S24）
#[test]
#[ignore = "実際にウィンドウを出す"]
fn close_button_forgets_text_after_menu_is_removed() {
    use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
    let editor = new_editor();
    editor.show_with(plain("secret"));
    // SAFETY: 自分のウィンドウに値を渡すだけ。WM_CLOSE はポインタを渡さない。
    unsafe {
        SetMenu(editor.hwnd, None).unwrap();
        SendMessageW(editor.hwnd, WM_CLOSE, None, None);
    }
    assert!(!editor.is_visible());
    assert_eq!(editor.text(), "");
}

// 監査 S30: 同じユーザーのプロセスが編集欄の ID を書き換えても、× で閉じれば中身が消える
#[test]
#[ignore = "実際にウィンドウを出す"]
fn close_button_forgets_text_after_edit_ids_change() {
    use windows::Win32::UI::WindowsAndMessaging::{GWLP_ID, WM_CLOSE};
    let editor = new_editor();
    editor.show_with(plain("secret"));
    // SAFETY: 自分のウィンドウに値を渡すだけ。WM_CLOSE はポインタを渡さない。
    unsafe {
        SetWindowLongPtrW(editor.edit, GWLP_ID, 7);
        SetWindowLongPtrW(editor.edit_nowrap, GWLP_ID, 8);
        SendMessageW(editor.hwnd, WM_CLOSE, None, None);
    }
    assert!(!editor.is_visible());
    assert_eq!(window_text(editor.edit), "");
    assert_eq!(window_text(editor.edit_nowrap), "");
}

// 監査 S30: 窓のデータ（GWLP_USERDATA）を書き換えられても、メニューバーの「お気に入り」は作り直せる
#[test]
#[ignore = "実際にウィンドウを出す"]
fn menu_bar_still_refreshes_after_userdata_is_overwritten() {
    use windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount;
    let editor = new_editor();
    // SAFETY: 自分のウィンドウとメニューに値を渡すだけ。WM_INITMENUPOPUP の wparam はメニューのハンドル。
    let items = unsafe {
        SetWindowLongPtrW(editor.hwnd, GWLP_USERDATA, 0x1234);
        SendMessageW(
            editor.hwnd,
            WM_INITMENUPOPUP,
            Some(WPARAM(editor.bar_menu.0 as usize)),
            Some(LPARAM(0)),
        );
        GetMenuItemCount(Some(editor.bar_menu))
    };
    // 作り直すと、少なくとも「追加」と「整理」の項目がある（作った直後は空）
    assert!(items >= 2, "項目の数 {items}");
}

// クリップボードも書き換える
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn commit_keeps_private_mark_of_opened_text() {
    let editor = new_editor();
    for private in [true, false] {
        editor.show_with(Ok(Some(ClipText {
            text: "P@ssw0rd".into(),
            private,
        })));
        assert!(editor.press(VK_RETURN.0, true, false, false));
        let written = clipboard::read_text().unwrap().unwrap();
        assert_eq!(written.text, "P@ssw0rd");
        assert_eq!(written.private, private);
    }
}

// 監査 S29: 編集欄の中のコピー・切り取りでも、開いた内容の「残さない」印を引き継ぐ
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn copy_and_cut_keep_private_mark_of_opened_text() {
    let editor = new_editor();
    for private in [true, false] {
        editor.show_with(Ok(Some(ClipText {
            text: "P@ssw0rd".into(),
            private,
        })));
        set_selection(editor.active(), 0, 4);
        send_to_edit(&editor, WM_COPY, 0);
        let copied = clipboard::read_text().unwrap().unwrap();
        assert_eq!(copied.text, "P@ss", "選んだところだけ");
        assert_eq!(copied.private, private, "コピー");

        send_to_edit(&editor, WM_CUT, 0);
        let cut = clipboard::read_text().unwrap().unwrap();
        assert_eq!(cut.text, "P@ss");
        assert_eq!(cut.private, private, "切り取り");
        assert_eq!(editor.text(), "w0rd", "切り取ったところは消える");
        assert!(can_undo(&editor), "切り取りは Ctrl+Z で戻せる");
    }
    editor.hide();
}

// 監査 S29: Ctrl+C のキー操作も、同じ経路で印を引き継ぐ
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn ctrl_c_key_keeps_private_mark() {
    let editor = new_editor();
    editor.show_with(Ok(Some(ClipText {
        text: "P@ssw0rd".into(),
        private: true,
    })));
    // 編集欄は Ctrl+C を、文字コード 3 の WM_CHAR として受け取る
    send_to_edit(&editor, WM_CHAR, 3);
    let copied = clipboard::read_text().unwrap().unwrap();
    assert_eq!(copied.text, "P@ssw0rd");
    assert!(copied.private);
    editor.hide();
}

// 監査 S29: 印の付いたものを貼り付けたら、書き込むときにも印を付ける
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn pasting_private_text_marks_the_content() {
    let editor = new_editor();
    editor.show_with(plain("ひな形"));
    put_caret_at_end(&editor);
    clipboard::write_text(None, " P@ss", true).unwrap();
    send_to_edit(&editor, WM_PASTE, 0);
    assert_eq!(editor.text(), "ひな形 P@ss");
    assert!(editor.press(VK_RETURN.0, true, false, false));
    let written = clipboard::read_text().unwrap().unwrap();
    assert_eq!(written.text, "ひな形 P@ss");
    assert!(written.private, "貼り付けた印を引き継ぐ");
}

// 監査 S29: 印のないものを貼り付けても、開いた内容の印は外さない
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn pasting_plain_text_keeps_private_mark() {
    let editor = new_editor();
    editor.show_with(Ok(Some(ClipText {
        text: "P@ss".into(),
        private: true,
    })));
    put_caret_at_end(&editor);
    clipboard::write_text(None, "word", false).unwrap();
    send_to_edit(&editor, WM_PASTE, 0);
    assert!(editor.press(VK_RETURN.0, true, false, false));
    let written = clipboard::read_text().unwrap().unwrap();
    assert_eq!(written.text, "P@ssword");
    assert!(written.private);
}

/// 表示している編集欄に、ポインタを渡さないメッセージを送る。
fn send_to_edit(editor: &Editor, msg: u32, wparam: usize) {
    // SAFETY: 自分の編集欄に、ポインタを渡さないメッセージを送るだけ。
    unsafe {
        SendMessageW(editor.active(), msg, Some(WPARAM(wparam)), Some(LPARAM(0)));
    }
}

fn put_caret_at_end(editor: &Editor) {
    let end = editor.text().encode_utf16().count();
    set_selection(editor.active(), end, end);
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn plain_backspace_is_left_to_edit_control() {
    let editor = new_editor();
    editor.show_with(plain("abc"));
    assert!(!editor.press(VK_BACK.0, false, false, false));
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn open_restores_minimized_window() {
    use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;
    let editor = new_editor();
    editor.show_with(plain("編集中"));
    // SAFETY: 自分のウィンドウに値を渡すだけ。
    unsafe {
        let _ = ShowWindow(editor.hwnd, SW_MINIMIZE);
    }
    editor.open(|| panic!("表示中は読み直さない"));
    // SAFETY: 値を渡すだけ。
    assert!(!unsafe { IsIconic(editor.hwnd) }.as_bool());
    assert_eq!(editor.text(), "編集中");
    editor.hide();
}

// 台帳 D6: 最小化したまま閉じても、次に開くときは元の大きさで、画面の中央に出る
#[test]
#[ignore = "実際にウィンドウを出す"]
fn closing_while_minimized_reopens_at_normal_size() {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SW_MINIMIZE};
    let editor = new_editor();
    editor.show_with(plain("一回目"));
    let mut expected = RECT::default();
    // SAFETY: 自分のウィンドウに値を渡し、RECT はローカル変数に受け取る。
    unsafe {
        let _ = GetWindowRect(editor.hwnd, &mut expected);
        let _ = ShowWindow(editor.hwnd, SW_MINIMIZE);
        SendMessageW(editor.hwnd, WM_CLOSE, None, None);
    }
    assert!(!editor.is_visible());
    editor.open(|| plain("二回目"));
    let mut actual = RECT::default();
    // SAFETY: 同上。
    unsafe {
        let _ = GetWindowRect(editor.hwnd, &mut actual);
    }
    // SAFETY: 値を渡すだけ。
    assert!(!unsafe { IsIconic(editor.hwnd) }.as_bool(), "最小化のまま");
    assert_eq!(
        (actual.left, actual.top, actual.right, actual.bottom),
        (expected.left, expected.top, expected.right, expected.bottom)
    );
    assert_eq!(editor.text(), "二回目");
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを作る"]
fn window_has_app_icons_sized_for_its_dpi() {
    use windows::Win32::UI::WindowsAndMessaging::WM_GETICON;
    let editor = new_editor();
    for (kind, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
        // SAFETY: 値を渡すだけ。WM_GETICON はウィンドウが持つアイコンを返す（破棄しない）。
        let (icon, expected) = unsafe {
            let icon = SendMessageW(editor.hwnd, WM_GETICON, Some(WPARAM(kind as usize)), None);
            let dpi = GetDpiForWindow(editor.hwnd);
            (icon, GetSystemMetricsForDpi(metric, dpi))
        };
        assert_ne!(icon.0, 0, "アイコンが付いていない");
        assert_eq!(crate::icon::icon_width(HICON(icon.0 as *mut _)), expected);
    }
}

#[test]
fn keys_map_to_actions() {
    use KeyAction::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_NUMPAD3, VK_PROCESSKEY};
    assert_eq!(key_action(VK_RETURN.0, true, false), Some(Commit));
    assert_eq!(key_action(VK_RETURN.0, false, false), None);
    assert_eq!(key_action(VK_ESCAPE.0, false, false), Some(Hide));
    assert_eq!(key_action(b'1' as u16, true, false), Some(Recall(0)));
    assert_eq!(key_action(b'9' as u16, true, false), Some(Recall(8)));
    assert_eq!(
        key_action(VK_NUMPAD3.0, true, false),
        Some(Recall(2)),
        "テンキーも同じ"
    );
    assert_eq!(key_action(b'0' as u16, true, false), None);
    assert_eq!(
        key_action(b'1' as u16, false, false),
        None,
        "Ctrl なしの数字は普通に入力する"
    );
    // Ctrl+Space は IME のオン／オフに使われていることがあるので、ClipEdit では使わない（メニューは Alt+F）
    assert_eq!(key_action(VK_SPACE.0, true, false), None);
    assert_eq!(key_action(VK_SPACE.0, false, false), None);
    assert_eq!(
        key_action(VK_PROCESSKEY.0, true, false),
        None,
        "IME の変換中は反応しない"
    );
}

#[test]
fn alt_with_ctrl_is_altgr_and_does_not_recall() {
    use KeyAction::*;
    assert_eq!(key_action(b'7' as u16, true, true), None, "AltGr+7");
    assert_eq!(key_action(b'7' as u16, true, false), Some(Recall(6)));
    // 既存のキーは Alt の有無に関わらず変えない
    assert_eq!(key_action(b'A' as u16, true, true), Some(SelectAll));
    assert_eq!(key_action(VK_RETURN.0, true, true), Some(Commit));
    assert_eq!(key_action(VK_BACK.0, true, true), Some(DeleteWord));
}

#[test]
fn repeat_flag_is_bit_30() {
    assert!(!is_repeat(0x0001_0001));
    assert!(is_repeat(0x4001_0001));
}

#[test]
fn favorite_title_names_the_favorite() {
    assert!(favorite_title("要約").starts_with("ClipEdit - お気に入り「要約」"));
}

fn favorites_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("clipedit-editor-favorites-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn selection(editor: &Editor) -> (u32, u32) {
    let (mut start, mut end) = (0u32, 0u32);
    // SAFETY: EM_GETSEL には、ローカル変数 start と end（u32）の場所を渡す。
    unsafe {
        SendMessageW(
            editor.active(),
            EM_GETSEL,
            Some(WPARAM(&mut start as *mut u32 as usize)),
            Some(LPARAM(&mut end as *mut u32 as isize)),
        );
    }
    (start, end)
}

fn file_count(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn recall_replaces_text_and_ctrl_z_restores() {
    use windows::Win32::UI::Controls::EM_UNDO;
    let dir = favorites_dir("recall");
    std::fs::write(dir.join("01 a.txt"), "プロンプトA\nつづき").unwrap();
    let editor = new_editor();
    editor.show_with(plain("コピーした文"));
    assert!(editor.recall(&dir, 0));
    assert_eq!(editor.text(), "プロンプトA\r\nつづき");
    assert_eq!(title_of(&editor), favorite_title("a"));
    assert_eq!(selection(&editor), (0, 0), "選択なしで先頭にキャレット");
    // SAFETY: EM_UNDO はポインタを渡さない。
    unsafe {
        SendMessageW(editor.edit, EM_UNDO, None, None);
    }
    assert_eq!(editor.text(), "コピーした文");
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn menu_bar_has_favorites_menu() {
    use windows::Win32::UI::WindowsAndMessaging::GetMenu;
    let editor = new_editor();
    // SAFETY: 自分のウィンドウに値を渡すだけ。
    let bar = unsafe { GetMenu(editor.hwnd) };
    assert_eq!(
        crate::favorite_menu::tests::menu_text(bar, 0),
        "お気に入り(&F)"
    );
}

// メニューバーの「お気に入り」は、開くたびにフォルダを読み直す
#[test]
#[ignore = "実際にウィンドウを出す"]
fn menu_bar_lists_favorites_each_time_it_opens() {
    let dir = favorites_dir("menu-bar-list");
    std::fs::write(dir.join("01 a.txt"), "A").unwrap();
    std::fs::write(dir.join("02 b.txt"), "B").unwrap();
    let editor = new_editor();
    editor.refresh_menu_bar(&dir);
    let text = |pos| crate::favorite_menu::tests::menu_text(editor.bar_menu, pos);
    assert_eq!(text(0), "&1  a");
    assert_eq!(text(1), "&2  b");
    assert_eq!(text(3), "今の内容をお気に入りに追加");
    std::fs::remove_file(dir.join("01 a.txt")).unwrap();
    editor.refresh_menu_bar(&dir);
    assert_eq!(text(0), "&1  b");
}

// 選ばれた項目は、メニューを開いたときの一覧で処理する（開いたあとにファイルが消えても、見えていた本文を使う）
#[test]
#[ignore = "実際にウィンドウを出す"]
fn menu_bar_command_uses_the_listing_that_was_shown() {
    let dir = favorites_dir("menu-bar-command");
    std::fs::write(dir.join("01 a.txt"), "プロンプトA").unwrap();
    let editor = new_editor();
    editor.show_with(plain("コピーした文"));
    editor.refresh_menu_bar(&dir);
    std::fs::remove_file(dir.join("01 a.txt")).unwrap();
    editor.menu_bar_command(&dir, crate::favorite_menu::CMD_FAVORITE_FIRST);
    assert_eq!(editor.text(), "プロンプトA");
    assert_eq!(title_of(&editor), favorite_title("a"));
    editor.hide();
}

// 同じお気に入りを続けて呼んでも、元に戻す履歴の「元の内容」が上書きされない
#[test]
#[ignore = "実際にウィンドウを出す"]
fn recalling_the_same_favorite_twice_keeps_the_original_in_undo() {
    use windows::Win32::UI::Controls::EM_UNDO;
    let dir = favorites_dir("recall-twice");
    std::fs::write(dir.join("01 a.txt"), "プロンプトA").unwrap();
    let editor = new_editor();
    editor.show_with(plain("コピーした文"));
    assert!(editor.recall(&dir, 0));
    assert!(editor.recall(&dir, 0));
    assert_eq!(editor.text(), "プロンプトA");
    // SAFETY: EM_UNDO はポインタを渡さない。
    unsafe {
        SendMessageW(editor.edit, EM_UNDO, None, None);
    }
    assert_eq!(editor.text(), "コピーした文");
    editor.hide();
}

// 「残さない」印つきの内容から、長い行のお気に入り（折り返さない編集欄）に入れ替えても、どちらの編集欄も元に戻せない
#[test]
#[ignore = "実際にウィンドウを出す"]
fn private_to_long_line_favorite_cannot_be_undone_in_either_edit() {
    use windows::Win32::UI::Controls::EM_CANUNDO;
    let dir = favorites_dir("private-long");
    std::fs::write(dir.join("01 long.txt"), "x".repeat(WRAP_LIMIT + 1)).unwrap();
    let editor = new_editor();
    editor.show_with(Ok(Some(ClipText {
        text: "secret".into(),
        private: true,
    })));
    assert!(editor.recall(&dir, 0));
    assert_eq!(editor.active(), editor.edit_nowrap);
    for edit in [editor.edit, editor.edit_nowrap] {
        // SAFETY: EM_CANUNDO はポインタを渡さない。
        let undo = unsafe { SendMessageW(edit, EM_CANUNDO, None, None) };
        assert_eq!(undo.0, 0, "元に戻せない");
    }
    assert!(!PRIVATE.with(|p| p.get()));
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn recall_out_of_range_keeps_text() {
    let dir = favorites_dir("out-of-range");
    std::fs::write(dir.join("01 a.txt"), "プロンプトA").unwrap();
    let editor = new_editor();
    editor.show_with(plain("元の文"));
    assert!(!editor.recall(&dir, 4));
    assert_eq!(editor.text(), "元の文");
    let empty = favorites_dir("out-of-range-empty");
    assert!(!editor.recall(&empty, 0));
    assert_eq!(editor.text(), "元の文");
    editor.hide();
}

// 入れ替える前が「残さない」印つきなら、Ctrl+Z でパスワードを印なしの状態に戻せないようにする
#[test]
#[ignore = "実際にウィンドウを出す"]
fn recall_after_private_text_clears_undo_and_private() {
    let dir = favorites_dir("private-recall");
    std::fs::write(dir.join("01 a.txt"), "プロンプトA").unwrap();
    let editor = new_editor();
    editor.show_with(Ok(Some(ClipText {
        text: "secret".into(),
        private: true,
    })));
    assert!(editor.recall(&dir, 0));
    assert!(!can_undo(&editor), "元に戻すで secret が出てこない");
    assert!(!PRIVATE.with(|p| p.get()));
    assert_eq!(
        editor.add_favorite(&dir),
        Ok("お気に入りの 2 番に追加しました".to_string()),
        "通知に本文の1行目（名前）を出さない（監査 S35）"
    );
    let added = std::fs::read_to_string(dir.join("02 プロンプトA.txt")).unwrap();
    assert_eq!(added, "プロンプトA");
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn long_line_favorite_uses_the_nowrap_editor() {
    let dir = favorites_dir("long-line");
    let long = "x".repeat(WRAP_LIMIT + 1);
    std::fs::write(dir.join("01 long.txt"), &long).unwrap();
    let editor = new_editor();
    editor.show_with(plain("短い文"));
    assert!(editor.recall(&dir, 0));
    assert_eq!(editor.active(), editor.edit_nowrap);
    assert_eq!(editor.text(), long);
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn open_favorite_when_hidden_shows_it() {
    let editor = new_editor();
    assert!(!editor.is_visible());
    editor.open_favorite(&Favorite {
        name: "a".into(),
        text: "本文".into(),
    });
    assert!(editor.is_visible());
    assert_eq!(editor.text(), "本文");
    assert_eq!(title_of(&editor), favorite_title("a"));
    assert_eq!(selection(&editor), (0, 0), "何も選ばず、先頭にキャレット");
    editor.hide();
}

#[test]
fn long_line_note_is_added_only_when_long() {
    assert_eq!(note_long_line("t".into(), false), "t");
    assert_eq!(
        note_long_line("t".into(), true),
        "t  (長い行があるため折り返さずに表示)"
    );
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn add_favorite_refuses_private_text() {
    let dir = favorites_dir("refuse-private");
    let editor = new_editor();
    editor.show_with(Ok(Some(ClipText {
        text: "P@ssw0rd".into(),
        private: true,
    })));
    assert_eq!(
        editor.add_favorite(&dir),
        Err("パスワード管理ソフトなどがコピーした内容は、お気に入りに追加できません".into())
    );
    assert_eq!(file_count(&dir), 0, "ディスクに書かない");
    editor.hide();
}

#[test]
#[ignore = "実際にウィンドウを出す"]
fn add_favorite_saves_current_text() {
    let dir = favorites_dir("add");
    let editor = new_editor();
    editor.show_with(plain("新しいプロンプト"));
    assert_eq!(
        editor.add_favorite(&dir),
        Ok("お気に入りの 1 番に追加しました".to_string()),
        "通知に本文の1行目（名前）を出さない（監査 S35）"
    );
    assert_eq!(editor.text(), "新しいプロンプト", "編集欄の中身はそのまま");
    assert!(editor.is_visible(), "開いたまま");
    editor.hide();
}
