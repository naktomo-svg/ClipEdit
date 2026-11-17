//! 編集欄のコピー・切り取り・貼り付けで、「残さない」印を引き継ぐ（監査 S29）。
//! 標準の編集欄は `CF_UNICODETEXT` しか扱わないので、Ctrl+C・Ctrl+X・Ctrl+V・Ctrl+Insert・
//! Shift+Insert・Shift+Delete・右クリックのメニューが行き着く `WM_COPY`・`WM_CUT`・`WM_PASTE` を、ここで処理する。
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_COPY, WM_CUT, WM_NCDESTROY, WM_PASTE};

use super::{PRIVATE, replace_selection, selection, window_text};
use crate::clipboard;
use crate::textops::normalize_newlines;

const SUBCLASS_ID: usize = 1;

/// 編集欄 edit のコピー・切り取り・貼り付けを、このファイルの処理に通す。
pub(super) fn attach(edit: HWND) -> windows::core::Result<()> {
    // SAFETY: edit はこのスレッドで作った自分の編集欄。edit_proc は extern "system" の関数で、
    // WM_NCDESTROY で外す。参照するデータは渡さない（0）。
    unsafe { SetWindowSubclass(edit, Some(edit_proc), SUBCLASS_ID, 0) }.ok()
}

extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_COPY | WM_CUT => {
            copy(hwnd, msg == WM_CUT);
            LRESULT(0)
        }
        WM_PASTE => {
            paste(hwnd);
            LRESULT(0)
        }
        _ => {
            if msg == WM_NCDESTROY {
                // SAFETY: attach で付けたものを、同じ関数と ID で外す。
                unsafe {
                    let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID);
                }
            }
            // SAFETY: 受け取った引数を、そのまま次の処理に渡す。
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
    }
}

/// 選んでいるところを、開いた内容の印を付けてクリップボードに置く。cut なら、置けたときだけ消す。
fn copy(edit: HWND, cut: bool) {
    let (start, end) = selection(edit);
    if start == end {
        return;
    }
    let units: Vec<u16> = window_text(edit).encode_utf16().collect();
    let end = end.min(units.len());
    let text = String::from_utf16_lossy(&units[start.min(end)..end]);
    let private = PRIVATE.with(|p| p.get());
    if clipboard::write_text(Some(edit), &text, private).is_ok() && cut {
        replace_selection(edit, "");
    }
}

/// クリップボードのテキストを、元に戻せる形で貼る。印が付いていれば、開いている内容も印付きにする。
fn paste(edit: HWND) {
    let Ok(Some(clip)) = clipboard::read_text() else {
        return;
    };
    if clip.private {
        PRIVATE.with(|p| p.set(true));
    }
    replace_selection(edit, &normalize_newlines(&clip.text));
}
