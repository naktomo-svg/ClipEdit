use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    HICON, IDI_APPLICATION, LR_DEFAULTCOLOR, LoadIconW, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, PostMessageW, SM_CXSMICON, SetForegroundWindow, TPM_RIGHTBUTTON, TrackPopupMenu,
    WM_APP, WM_NULL,
};
use windows::core::{Error, PCWSTR, w};

/// トレイアイコンから本体ウィンドウに届くメッセージ。
pub const WM_TRAY: u32 = WM_APP + 1;
pub const CMD_OPEN: usize = 1;
pub const CMD_AUTOSTART: usize = 2;
pub const CMD_EXIT: usize = 3;
pub const CMD_RELOAD: usize = 4;

const ICON_ID: u32 = 1;

/// 固定長の UTF-16 配列に文字列を詰める（はみ出す分は文字の単位で切り捨て、末尾は必ず 0）。
pub fn copy_wide(dst: &mut [u16], src: &str) {
    let mut i = 0;
    for c in src.chars() {
        let len = c.len_utf16();
        if i + len >= dst.len() {
            break;
        }
        c.encode_utf16(&mut dst[i..i + len]);
        i += len;
    }
    dst[i] = 0;
}

/// tools/make-icon.ps1 で作ったアイコン（PNG 形式の .ico）。
const ICON_DATA: &[u8] = include_bytes!("../assets/clipedit.ico");

/// .ico の中から size に合う画像を選び、(データの位置, 長さ) を返す。
/// 同じ大きさが無ければ次に大きいもの、それも無ければいちばん大きいもの。
fn pick_icon_image(data: &[u8], size: i32) -> Option<(usize, usize)> {
    let read_u16 = |at: usize| Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let read_u32 = |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let count = read_u16(4)? as usize;
    let mut best: Option<(i32, usize, usize)> = None;
    for i in 0..count {
        let entry = 6 + 16 * i;
        let width = match *data.get(entry)? {
            0 => 256,
            w => w as i32,
        };
        let len = read_u32(entry + 8)? as usize;
        let offset = read_u32(entry + 12)? as usize;
        data.get(offset..offset + len)?;
        let better = match best {
            None => true,
            Some((b, _, _)) if b < size => width > b,
            Some((b, _, _)) => width >= size && width < b,
        };
        if better {
            best = Some((width, offset, len));
        }
    }
    best.map(|(_, offset, len)| (offset, len))
}

/// 埋め込んだアイコンを size × size で読み込む。
pub fn load_app_icon(size: i32) -> windows::core::Result<HICON> {
    let (offset, len) = pick_icon_image(ICON_DATA, size).ok_or_else(|| Error::from(E_FAIL))?;
    // SAFETY: pick_icon_image は ICON_DATA の範囲内だけを返す（範囲外ならスライスがパニックする）。
    // CreateIconFromResourceEx は渡したスライスを読むだけで、作ったアイコンは呼び出し側が破棄する。
    unsafe {
        CreateIconFromResourceEx(
            &ICON_DATA[offset..offset + len],
            true,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
}

fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

pub fn add(hwnd: HWND, tip: &str) -> Result<(), String> {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    // トレイの小さいアイコンの大きさは表示スケールで変わる（100% で 16px、150% で 24px）
    // SAFETY: 値だけを渡す。hwnd が無効なら DPI は 0 になり、既定の大きさが返るだけ。
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd)) };
    let own_icon = load_app_icon(size).ok();
    data.hIcon = match own_icon {
        Some(icon) => icon,
        // SAFETY: システム定義のアイコンを読むだけ。共有アイコンなので破棄しない。
        None => unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|e| e.message())?,
    };
    copy_wide(&mut data.szTip, tip);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW で、文字列はどれも copy_wide で 0 終端にしてある。
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
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
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

/// トレイのツールチップを差し替える。
pub fn set_tip(hwnd: HWND, tip: &str) -> bool {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_TIP;
    copy_wide(&mut data.szTip, tip);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW で、文字列は copy_wide で 0 終端にしてある。
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool()
}

/// 通知の種類。うまくいった知らせは Info、問題を知らせるときは Warning（台帳 D34）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
}

/// トレイから通知バルーンを出す。トレイにアイコンが無いなどで出せなければ false。
pub fn notify(hwnd: HWND, kind: NoticeKind, title: &str, message: &str) -> bool {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = match kind {
        NoticeKind::Info => NIIF_INFO,
        NoticeKind::Warning => NIIF_WARNING,
    };
    copy_wide(&mut data.szInfoTitle, title);
    copy_wide(&mut data.szInfo, message);
    // SAFETY: data は cbSize を設定した NOTIFYICONDATAW で、文字列は copy_wide で 0 終端にしてある。
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool()
}

/// 出せなかった通知を覚えておき、あとで送り直す（台帳 D3）。
/// ログイン直後の自動起動では、トレイの準備ができる前に通知を出そうとして失敗することがある。
#[derive(Default)]
pub struct PendingNotices(Vec<(NoticeKind, String, String)>);

impl PendingNotices {
    /// send で送り、失敗したら覚えておく。
    pub fn post(
        &mut self,
        kind: NoticeKind,
        title: &str,
        message: &str,
        send: impl FnOnce(NoticeKind, &str, &str) -> bool,
    ) {
        if !send(kind, title, message) {
            self.0.push((kind, title.to_string(), message.to_string()));
        }
    }

    /// 覚えている通知を順に送り直す。また失敗したものは残す。
    pub fn retry(&mut self, mut send: impl FnMut(NoticeKind, &str, &str) -> bool) {
        self.0
            .retain(|(kind, title, message)| !send(*kind, title, message));
    }
}

/// アイコンの画像の幅（テストで、作ったアイコンの大きさを確かめる）。
#[cfg(test)]
pub fn icon_width(icon: HICON) -> i32 {
    use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};
    // SAFETY: icon は有効なアイコン。bitmap には BITMAP の大きさを渡す。
    // GetIconInfo が作ったビットマップ2つは、ここで破棄する（アイコンは破棄しない）。
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &mut info).unwrap();
        let mut bitmap = BITMAP::default();
        GetObjectW(
            info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut BITMAP as *mut _),
        );
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
        bitmap.bmWidth
    }
}

/// 右クリックメニューを出す。選ばれた項目は `WM_COMMAND`（`CMD_*`）で hwnd に届く。
pub fn show_menu(hwnd: HWND, autostart_on: bool) {
    // SAFETY: 項目名は w! で作った 0 終端の文字列（区切り線だけは null でよい）。
    // pt はこの関数のローカル変数。作ったメニューは TrackPopupMenu が戻った後に必ず破棄する。
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = if autostart_on {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("開く"));
        let _ = AppendMenuW(
            menu,
            MF_STRING | check,
            CMD_AUTOSTART,
            w!("ログイン時に起動"),
        );
        let _ = AppendMenuW(menu, MF_STRING, CMD_RELOAD, w!("設定を読み直す"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("終了"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
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

    fn entry_width(size: i32) -> u8 {
        let (offset, _) = pick_icon_image(ICON_DATA, size).unwrap();
        // PNG の IHDR にある幅（16〜19 バイト目、ビッグエンディアン）
        ICON_DATA[offset + 19]
    }

    #[test]
    fn picks_exact_size_when_available() {
        assert_eq!(entry_width(16), 16);
        assert_eq!(entry_width(24), 24);
        assert_eq!(entry_width(40), 40);
    }

    #[test]
    fn picks_next_larger_size_otherwise() {
        assert_eq!(entry_width(18), 20);
        assert_eq!(entry_width(30), 32);
    }

    #[test]
    fn picks_largest_when_request_exceeds_all() {
        assert_eq!(entry_width(64), 48);
    }

    #[test]
    fn width_zero_means_256_pixels() {
        // .ico の項目の幅 0 は 256px を表す。ICONDIR（6 バイト）＋項目1つ（16 バイト）＋データ4バイト
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&4u32.to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend_from_slice(&[1, 2, 3, 4]);
        // 256px が欲しいとき、ちょうど同じ大きさとして選ばれる（48px などより優先される）
        let mut two = ico.clone();
        two[4] = 2;
        two.splice(22..22, [48, 48, 0, 0, 1, 0, 32, 0, 4, 0, 0, 0, 42, 0, 0, 0]);
        two[18] = 38; // 1つ目の項目のデータ位置を、2つ目の項目の後ろへずらす
        two.extend_from_slice(&[5, 6, 7, 8]);
        assert_eq!(pick_icon_image(&ico, 256), Some((22, 4)));
        assert_eq!(pick_icon_image(&two, 256), Some((38, 4)));
        assert_eq!(pick_icon_image(&two, 48), Some((42, 4)));
    }

    #[test]
    fn rejects_broken_icon_data() {
        assert_eq!(pick_icon_image(&[0, 0, 1], 16), None);
        assert_eq!(
            pick_icon_image(&[0, 0, 1, 0, 1, 0], 16),
            None,
            "項目が途中で切れている"
        );
    }

    #[test]
    fn loads_app_icon_at_requested_size() {
        for size in [16, 24, 32] {
            let icon = load_app_icon(size).unwrap();
            assert_eq!(icon_width(icon), size);
            // SAFETY: icon は今作った自分のアイコンで、このあと使わない。
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }

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

    #[test]
    fn copy_wide_fits_and_terminates() {
        let mut buf = [0xFFFFu16; 8];
        copy_wide(&mut buf, "abc");
        assert_eq!(&buf[..4], &[b'a' as u16, b'b' as u16, b'c' as u16, 0]);
    }

    // 台帳 D35: 切るときに絵文字などのサロゲートペアの片方だけを残さない
    #[test]
    fn copy_wide_does_not_split_surrogate_pair() {
        let mut buf = [0xFFFFu16; 4];
        copy_wide(&mut buf, "ab😀");
        let expected: Vec<u16> = "ab\0".encode_utf16().collect();
        assert_eq!(buf[..3], expected[..]);
    }

    #[test]
    fn copy_wide_truncates_long_text() {
        let mut buf = [0xFFFFu16; 4];
        copy_wide(&mut buf, "abcdef");
        assert_eq!(buf, [b'a' as u16, b'b' as u16, b'c' as u16, 0]);
    }
}
