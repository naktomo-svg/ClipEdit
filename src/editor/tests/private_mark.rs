//! 編集欄の中のコピー・切り取り・貼り付けと Ctrl+Enter が、「残さない」印を引き継ぐことのテスト（監査 S29）。
//! 実際にウィンドウを出し、クリップボードを書き換えるので通常は実行しない: cargo test -- --ignored --test-threads=1
use super::*;

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

/// 別のスレッドでクリップボードを開いたままにする。その間の書き込みは、試し直しのあとで失敗する。drop で閉じる。
struct ClipboardHeld {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ClipboardHeld {
    fn new() -> Self {
        use std::sync::mpsc::channel;
        use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};
        let (opened_tx, opened_rx) = channel();
        let (release, release_rx) = channel::<()>();
        let thread = std::thread::spawn(move || {
            // SAFETY: このスレッドで開き、同じスレッドで閉じる。
            unsafe {
                OpenClipboard(None).unwrap();
                opened_tx.send(()).unwrap();
                let _ = release_rx.recv();
                CloseClipboard().unwrap();
            }
        });
        opened_rx.recv().unwrap();
        Self {
            release: Some(release),
            thread: Some(thread),
        }
    }
}

impl Drop for ClipboardHeld {
    fn drop(&mut self) {
        drop(self.release.take());
        // テストが失敗して巻き戻っている途中なら、ここでさらにパニックすると全体が止まるので、元の失敗だけを伝える（台帳 D77）
        if let Some(thread) = self.thread.take()
            && let Err(panic) = thread.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}

// 台帳 D67: 編集欄の中のコピー・切り取りでクリップボードに書けなければ、タイトルに理由を出す。切り取りは消さない
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn copy_and_cut_failures_show_in_the_title() {
    let editor = new_editor();
    editor.show_with(plain("abcdef"));
    let held = ClipboardHeld::new();
    set_selection(editor.active(), 0, 3);
    send_to_edit(&editor, WM_COPY, 0);
    let copy_title = title_of(&editor);
    send_to_edit(&editor, WM_CUT, 0);
    let cut_title = title_of(&editor);
    drop(held);
    assert!(
        copy_title.starts_with(&failure_title(TITLE, "コピー失敗", "")),
        "{copy_title}"
    );
    assert!(
        cut_title.starts_with(&failure_title(TITLE, "切り取り失敗", "")),
        "{cut_title}"
    );
    assert_eq!(editor.text(), "abcdef", "書けなかった切り取りは消さない");
    editor.show_with(plain("x"));
    assert_eq!(title_of(&editor), TITLE, "次に開くと元のタイトルに戻る");
    editor.hide();
}

// 台帳 D72: 長い行の編集欄でコピーに失敗しても、長い行の注記は消さない。あとで成功したら元のタイトルに戻す
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn copy_failure_keeps_the_long_line_note_and_clears_after_success() {
    let editor = new_editor();
    editor.show_with(plain(&"x".repeat(WRAP_LIMIT + 1)));
    let normal = note_long_line(TITLE.to_string(), true);
    set_selection(editor.active(), 0, 3);
    let held = ClipboardHeld::new();
    send_to_edit(&editor, WM_COPY, 0);
    let failed = title_of(&editor);
    drop(held);
    assert!(
        failed.starts_with(&failure_title(&normal, "コピー失敗", "")),
        "{failed}"
    );
    send_to_edit(&editor, WM_COPY, 0);
    assert_eq!(title_of(&editor), normal, "成功したら元に戻る");
    editor.hide();
}

// 台帳 D72: Ctrl+Enter の書き込みに失敗しても、長い行の注記は消さない
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn commit_failure_keeps_the_long_line_note() {
    let editor = new_editor();
    editor.show_with(plain(&"x".repeat(WRAP_LIMIT + 1)));
    let normal = note_long_line(TITLE.to_string(), true);
    let held = ClipboardHeld::new();
    assert!(editor.press(VK_RETURN.0, true, false, false));
    let failed = title_of(&editor);
    drop(held);
    assert!(
        failed.starts_with(&failure_title(&normal, "書き込み失敗", "")),
        "{failed}"
    );
    assert!(editor.is_visible(), "書けなければ閉じない");
    editor.hide();
}

// 台帳 D79: 失敗を出していなければ、コピーが成功してもタイトルを付け直さない（スクリーンリーダーに余分な変化を伝えない）
#[test]
#[ignore = "実際にウィンドウを出し、クリップボードを書き換える"]
fn successful_copy_without_a_failure_leaves_the_title_alone() {
    let editor = new_editor();
    editor.show_with(plain("abcdef"));
    // 付け直したかどうかを見るため、いつものタイトルとは違う目印を、set_title を通さずに付ける
    set_text(editor.hwnd, "目印");
    set_selection(editor.active(), 0, 3);
    send_to_edit(&editor, WM_COPY, 0);
    assert_eq!(title_of(&editor), "目印");
    editor.hide();
}
