use super::title::failure_title;
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

// 台帳 D67: Ctrl+Enter の書き込みも、編集欄の中のコピー・切り取りも、失敗したら同じ形でタイトルに出す
#[test]
fn failure_title_names_the_action_and_reason() {
    assert_eq!(
        failure_title("ClipEdit - お気に入り「a」", "コピー失敗", "開けません"),
        "ClipEdit - お気に入り「a」  - コピー失敗: 開けません"
    );
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "show_with にそのまま渡せる形で返す"
)]
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

// 台帳 D62: Ctrl+A は、折り返す編集欄でも長い行の編集欄でも全文を選ぶ
#[test]
#[ignore = "実際にウィンドウを出す"]
fn ctrl_a_selects_all_text_in_both_editors() {
    let editor = new_editor();
    let long = "x".repeat(WRAP_LIMIT + 1);
    for text in ["短い行\r\n2行目", long.as_str()] {
        editor.show_with(plain(text));
        set_selection(editor.active(), 0, 0);
        assert!(editor.press(u16::from(b'A'), true, false, false));
        let len = text.encode_utf16().count();
        assert_eq!(selection(editor.active()), (0, len), "{len} 文字");
        editor.hide();
    }
}

// 台帳 D62: 終わりの位置が大きすぎるときは、末尾までを選ぶ
#[test]
#[ignore = "実際にウィンドウを出す"]
fn set_selection_past_the_end_selects_to_the_end() {
    let editor = new_editor();
    editor.show_with(plain("abcdef"));
    set_selection(editor.active(), 2, usize::MAX);
    assert_eq!(selection(editor.active()), (2, 6));
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
        let _ = GetWindowRect(editor.hwnd, &raw mut expected);
        let _ = ShowWindow(editor.hwnd, SW_MINIMIZE);
        SendMessageW(editor.hwnd, WM_CLOSE, None, None);
    }
    assert!(!editor.is_visible());
    editor.open(|| plain("二回目"));
    let mut actual = RECT::default();
    // SAFETY: 同上。
    unsafe {
        let _ = GetWindowRect(editor.hwnd, &raw mut actual);
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
    assert_eq!(key_action(u16::from(b'1'), true, false), Some(Recall(0)));
    assert_eq!(key_action(u16::from(b'9'), true, false), Some(Recall(8)));
    assert_eq!(
        key_action(VK_NUMPAD3.0, true, false),
        Some(Recall(2)),
        "テンキーも同じ"
    );
    assert_eq!(key_action(u16::from(b'0'), true, false), None);
    assert_eq!(
        key_action(u16::from(b'1'), false, false),
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
    assert_eq!(key_action(u16::from(b'7'), true, true), None, "AltGr+7");
    assert_eq!(key_action(u16::from(b'7'), true, false), Some(Recall(6)));
    // 既存のキーは Alt の有無に関わらず変えない
    assert_eq!(key_action(u16::from(b'A'), true, true), Some(SelectAll));
    assert_eq!(key_action(VK_RETURN.0, true, true), Some(Commit));
    assert_eq!(key_action(VK_BACK.0, true, true), Some(DeleteWord));
}

#[test]
fn repeat_flag_is_bit_30() {
    assert!(!is_repeat(0x0001_0001));
    assert!(is_repeat(0x4001_0001));
}

#[test]
fn long_line_note_is_added_only_when_long() {
    assert_eq!(note_long_line("t".into(), false), "t");
    assert_eq!(
        note_long_line("t".into(), true),
        "t  (長い行があるため折り返さずに表示)"
    );
}

mod favorite_tests;
mod private_mark;
