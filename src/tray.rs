use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, IDI_APPLICATION,
    LoadIconW, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, PostMessageW,
    SM_CXSMICON, SetForegroundWindow, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_NULL,
};
use windows::core::{PCWSTR, w};

use crate::autostart;
use crate::favorite_menu;
use crate::favorites::Listing;

/// トレイアイコンから本体ウィンドウに届くメッセージ。
pub const WM_TRAY: u32 = WM_APP + 1;
pub const CMD_OPEN: usize = 1;
pub const CMD_AUTOSTART: usize = 2;
pub const CMD_EXIT: usize = 3;
pub const CMD_RELOAD: usize = 4;

const ICON_ID: u32 = 1;

fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

fn add(hwnd: HWND, tip: &str) -> Result<(), String> {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    // トレイの小さいアイコンの大きさは表示スケールで変わる（100% で 16px、150% で 24px）
    // SAFETY: 値だけを渡す。hwnd が無効なら DPI は 0 になり、既定の大きさが返るだけ。
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd)) };
    let own_icon = crate::icon::load_app_icon(size).ok();
    data.hIcon = match own_icon {
        Some(icon) => icon,
        // SAFETY: システム定義のアイコンを読むだけ。共有アイコンなので破棄しない。
        None => unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|e| e.message())?,
    };
    crate::wide::copy_wide(&mut data.szTip, tip);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW で、文字列はどれも copy_wide で 0 終端にしてある。
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) }.as_bool();
    if let Some(icon) = own_icon {
        // シェルはアイコンを複製して持つので、こちらの分は破棄してよい
        // SAFETY: icon は load_app_icon で作った自分のアイコンで、このあと使わない。
        unsafe {
            let _ = DestroyIcon(icon);
        }
    }
    if added {
        Ok(())
    } else {
        Err("トレイアイコンを追加できませんでした".to_string())
    }
}

pub fn remove(hwnd: HWND) {
    let data = base_data(hwnd);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW。登録が無ければ失敗が返るだけ。
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &raw const data);
    }
}

/// トレイのツールチップを差し替える。
pub fn set_tip(hwnd: HWND, tip: &str) -> bool {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_TIP;
    crate::wide::copy_wide(&mut data.szTip, tip);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW で、文字列は copy_wide で 0 終端にしてある。
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) }.as_bool()
}

/// 通知の種類。うまくいった知らせは Info、問題を知らせるときは Warning（台帳 D34）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
}

/// トレイから通知バルーンを出す。トレイにアイコンが無いなどで出せなければ false。
fn notify(hwnd: HWND, kind: NoticeKind, title: &str, message: &str) -> bool {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = match kind {
        NoticeKind::Info => NIIF_INFO,
        NoticeKind::Warning => NIIF_WARNING,
    };
    crate::wide::copy_wide(&mut data.szInfoTitle, title);
    crate::wide::copy_wide(&mut data.szInfo, message);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW で、文字列は copy_wide で 0 終端にしてある。
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) }.as_bool()
}

/// 出せなかった通知を覚えておき、あとで送り直す（台帳 D3）。
/// ログイン直後の自動起動では、トレイの準備ができる前に通知を出そうとして失敗することがある。
#[derive(Default)]
struct PendingNotices(Vec<(NoticeKind, String, String)>);

/// 覚えておく通知の数の上限。超えたら古いものから捨てる（監査 S34）。
const MAX_PENDING: usize = 5;

impl PendingNotices {
    /// send で送り、失敗したら覚えておく。同じ通知はもう覚えていれば足さず、`MAX_PENDING` を超えたら古いものを捨てる。
    fn post(
        &mut self,
        kind: NoticeKind,
        title: &str,
        message: &str,
        send: impl FnOnce(NoticeKind, &str, &str) -> bool,
    ) {
        if send(kind, title, message) {
            return;
        }
        let notice = (kind, title.to_string(), message.to_string());
        if self.0.contains(&notice) {
            return;
        }
        if self.0.len() >= MAX_PENDING {
            self.0.remove(0);
        }
        self.0.push(notice);
    }

    /// 覚えている通知を順に送り直す。また失敗したものは残す。
    fn retry(&mut self, mut send: impl FnMut(NoticeKind, &str, &str) -> bool) {
        self.0
            .retain(|(kind, title, message)| !send(*kind, title, message));
    }

    /// later の通知を、覚えているものの後ろに足す。同じものは重ねず、`MAX_PENDING` を超えたら古いものを捨てる。
    fn append(&mut self, later: PendingNotices) {
        for (kind, title, message) in later.0 {
            self.post(kind, &title, &message, |_, _, _| false);
        }
    }
}

thread_local! {
    static PENDING_NOTICES: RefCell<PendingNotices> = RefCell::new(PendingNotices::default());
}

// Shell_NotifyIconW の中で送られてきたメッセージが処理されると、そこからまた通知を出すことがある。
// PENDING_NOTICES を借りたまま呼ぶと二重に借りてパニックするので、呼ぶ間は借りない（監査 S38）

/// トレイにアイコンを出す。出せたら、それまでに出せなかった通知を送り直す（台帳 D3）。
pub fn add_icon(hwnd: HWND, tip: &str) -> bool {
    let added = add(hwnd, tip).is_ok();
    if added {
        let mut pending = PENDING_NOTICES.with(|p| std::mem::take(&mut *p.borrow_mut()));
        pending.retry(|k, t, m| notify(hwnd, k, t, m));
        PENDING_NOTICES.with(|p| {
            // 送り直している間に覚えた通知は、残ったものの後ろに足す
            let mut current = p.borrow_mut();
            pending.append(std::mem::take(&mut *current));
            *current = pending;
        });
    }
    added
}

/// 通知を出す。トレイの準備ができていなくて出せなければ、覚えておいてあとで出す。
pub fn show_notice(hwnd: HWND, kind: NoticeKind, title: &str, message: &str) {
    if !notify(hwnd, kind, title, message) {
        PENDING_NOTICES.with(|p| p.borrow_mut().post(kind, title, message, |_, _, _| false));
    }
}

/// 右クリックメニューを出す。選ばれた項目は `WM_COMMAND`（`CMD_*`）で hwnd に届く。
pub fn show_menu(hwnd: HWND, autostart: autostart::State, favorites: &Listing) {
    // SAFETY: 項目名は w! で作った 0 終端の文字列（区切り線だけは null でよい）。
    // pt はこの関数のローカル変数。作ったメニューは TrackPopupMenu が戻った後に必ず破棄する。
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = if autostart == autostart::State::On {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        // 別の場所の exe が登録されているときは、チェックなしのまま、そう書き添える（監査 S23）
        let autostart_label = if autostart == autostart::State::Elsewhere {
            w!("ログイン時に起動（別の場所の exe が登録されています）")
        } else {
            w!("ログイン時に起動")
        };
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("開く"));
        // サブメニューは親のメニューに付けるので、DestroyMenu(menu) でいっしょに破棄される
        // 付けられなかったサブメニューは親といっしょに消えないので、ここで破棄する（監査 S28・S38）
        if let Some(sub) = favorite_menu::build(&favorite_menu::items(favorites, false))
            && AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("お気に入り")).is_err()
        {
            let _ = DestroyMenu(sub);
        }
        let _ = AppendMenuW(menu, MF_STRING | check, CMD_AUTOSTART, autostart_label);
        let _ = AppendMenuW(menu, MF_STRING, CMD_RELOAD, w!("設定を読み直す"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("終了"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&raw mut pt);
        // これをしないとメニューの外をクリックしても閉じない（Win32 の既知の作法）
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 台帳 D3: トレイの準備前に送れなかった通知は、覚えておいて送り直す
    #[test]
    fn failed_notice_is_kept_and_sent_on_retry() {
        let mut pending = PendingNotices::default();
        pending.post(NoticeKind::Warning, "t1", "m1", |_, _, _| false);
        pending.post(NoticeKind::Warning, "t2", "m2", |_, _, _| true);
        pending.post(NoticeKind::Info, "t3", "m3", |_, _, _| false);
        let mut sent = Vec::new();
        pending.retry(|k, t, m| {
            sent.push(format!("{k:?}:{t}:{m}"));
            true
        });
        assert_eq!(
            sent,
            ["Warning:t1:m1", "Info:t3:m3"],
            "送れなかったものだけを、順番どおりに、種類もそのまま"
        );
        let mut again = 0;
        pending.retry(|_, _, _| {
            again += 1;
            true
        });
        assert_eq!(again, 0, "送れたものは残らない");
    }

    // トレイの準備前に出そうとした通知は、準備後にちょうど1回だけ出る
    #[test]
    fn notice_posted_before_tray_is_sent_once_after_it() {
        let mut pending = PendingNotices::default();
        pending.post(NoticeKind::Info, "t", "m", |_, _, _| false);
        let mut sent = Vec::new();
        pending.retry(|_, t, m| {
            sent.push((t.to_string(), m.to_string()));
            true
        });
        pending.retry(|_, t, m| {
            sent.push((t.to_string(), m.to_string()));
            true
        });
        assert_eq!(sent, vec![("t".to_string(), "m".to_string())]);
    }

    #[test]
    fn notice_failing_again_stays_pending() {
        let mut pending = PendingNotices::default();
        pending.post(NoticeKind::Warning, "t", "m", |_, _, _| false);
        pending.retry(|_, _, _| false);
        let mut sent = 0;
        pending.retry(|_, _, _| {
            sent += 1;
            true
        });
        assert_eq!(sent, 1);
    }

    // 監査 S34: トレイがない間に通知が続いても、覚えるのは新しいほうから決まった数だけ
    #[test]
    fn pending_notices_keep_only_the_latest() {
        let mut pending = PendingNotices::default();
        for i in 0..MAX_PENDING + 3 {
            pending.post(NoticeKind::Info, "t", &i.to_string(), |_, _, _| false);
        }
        let mut sent = Vec::new();
        pending.retry(|_, _, m| {
            sent.push(m.to_string());
            true
        });
        let expected: Vec<String> = (3..MAX_PENDING + 3).map(|i| i.to_string()).collect();
        assert_eq!(sent, expected);
    }

    // 監査 S34: 同じ通知は1つだけ覚える
    #[test]
    fn same_pending_notice_is_kept_once() {
        let mut pending = PendingNotices::default();
        for _ in 0..3 {
            pending.post(NoticeKind::Warning, "t", "m", |_, _, _| false);
        }
        pending.post(NoticeKind::Info, "t", "m", |_, _, _| false);
        let mut sent = Vec::new();
        pending.retry(|k, _, _| {
            sent.push(k);
            true
        });
        assert_eq!(sent, vec![NoticeKind::Warning, NoticeKind::Info]);
    }

    fn messages(pending: &mut PendingNotices) -> Vec<String> {
        let mut sent = Vec::new();
        pending.retry(|_, _, m| {
            sent.push(m.to_string());
            true
        });
        sent
    }

    // 台帳 D64: 送り直している間に覚えた通知は、残ったものの後ろに足す。同じものは重ねない
    #[test]
    fn notices_queued_during_retry_go_after_the_rest() {
        let mut pending = PendingNotices::default();
        for m in ["A", "B"] {
            pending.post(NoticeKind::Info, "t", m, |_, _, _| false);
        }
        pending.retry(|_, _, m| m == "B");
        let mut later = PendingNotices::default();
        for m in ["C", "A"] {
            later.post(NoticeKind::Info, "t", m, |_, _, _| false);
        }
        pending.append(later);
        assert_eq!(messages(&mut pending), ["A", "C"]);
    }

    // 台帳 D64: 足したあとも、覚えるのは新しいほうから MAX_PENDING 件まで
    #[test]
    fn notices_queued_during_retry_respect_the_limit() {
        let mut pending = PendingNotices::default();
        for i in 0..MAX_PENDING {
            pending.post(NoticeKind::Info, "t", &format!("old{i}"), |_, _, _| false);
        }
        let mut later = PendingNotices::default();
        later.post(NoticeKind::Info, "t", "new", |_, _, _| false);
        pending.append(later);
        let sent = messages(&mut pending);
        assert_eq!(sent.len(), MAX_PENDING);
        assert_eq!(
            sent.first().map(String::as_str),
            Some("old1"),
            "いちばん古いものを捨てる"
        );
        assert_eq!(sent.last().map(String::as_str), Some("new"));
    }
}
