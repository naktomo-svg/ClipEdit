//! 編集ウィンドウのタイトル。いつものタイトル（読み込んだ内容・お気に入りの名前・長い行の注記）を覚えておき、
//! 書き込みの失敗はその後ろに足し、次に成功したら戻す（台帳 D67・D72・D79）。
use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::HWND;

use super::{TITLE, set_text};
use crate::clipboard::ClipText;

thread_local! {
    // 失敗を出す前の、いつものタイトル（長い行の注記やお気に入りの名前を含む）。失敗の表示はこの後ろに足し、
    // 次に成功したらこれに戻す（台帳 D72）。スレッドに1つなので、編集ウィンドウは1スレッドに1つの前提（PRIVATE と同じ）
    static BASE_TITLE: RefCell<String> = const { RefCell::new(String::new()) };
    // 今タイトルに失敗を出しているか。出していないときは、成功しても付け直さない（台帳 D79）
    static FAILURE_SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// お気に入りに入れ替えたときのウィンドウタイトル。
pub(super) fn favorite_title(name: &str) -> String {
    format!("ClipEdit - お気に入り「{name}」  (Ctrl+Enter: クリップボードに書き込む / Esc: 閉じる)")
}

/// 長い行があるときに、折り返さずに表示していることをタイトルに書き足す。
pub(super) fn note_long_line(mut title: String, long: bool) -> String {
    if long {
        title.push_str("  (長い行があるため折り返さずに表示)");
    }
    title
}

/// 書き込みに失敗したときのタイトル。何に失敗したか（action）と理由を、いつものタイトル base の後ろに付ける。
pub(super) fn failure_title(base: &str, action: &str, reason: &str) -> String {
    format!("{base}  - {action}: {reason}")
}

/// window のタイトルを、いつものタイトルの後ろに失敗（action と理由）を足したものにする。
pub(super) fn show_failure(window: HWND, action: &str, reason: &str) {
    let title = BASE_TITLE.with(|b| failure_title(&b.borrow(), action, reason));
    FAILURE_SHOWN.with(|f| f.set(true));
    set_text(window, &title);
}

/// 失敗を出していれば、window のタイトルをいつものタイトルに戻す。出していなければ何もしない。
pub(super) fn restore_title(window: HWND) {
    if !FAILURE_SHOWN.with(|f| f.replace(false)) {
        return;
    }
    let title = BASE_TITLE.with(|b| b.borrow().clone());
    set_text(window, &title);
}

/// 読み込み結果に応じたウィンドウタイトル。
pub fn title_for(content: &Result<Option<ClipText>, String>) -> String {
    match content {
        Ok(Some(_)) => TITLE.to_string(),
        Ok(None) => format!("{TITLE}  (テキストなし)"),
        Err(e) => format!("{TITLE}  - {e}"),
    }
}

/// window にいつものタイトルを付けて覚える。失敗の表示（`show_failure`）はこの後ろに足す。
pub(super) fn set(window: HWND, title: &str) {
    BASE_TITLE.with(|b| title.clone_into(&mut b.borrow_mut()));
    FAILURE_SHOWN.with(|f| f.set(false));
    set_text(window, title);
}
