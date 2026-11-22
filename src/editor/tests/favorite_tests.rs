//! 編集ウィンドウのお気に入り（Ctrl+1〜9 の入れ替え・メニューバー・追加）のテスト。
use super::*;

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
            Some(WPARAM(&raw mut start as usize)),
            Some(LPARAM(&raw mut end as isize)),
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
    assert!(!PRIVATE.with(std::cell::Cell::get));
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
    assert!(!PRIVATE.with(std::cell::Cell::get));
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
