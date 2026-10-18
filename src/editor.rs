use std::cell::Cell;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject, FF_MODERN,
    FIXED_PITCH, FW_NORMAL, GetMonitorInfoW, HFONT, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, OUT_DEFAULT_PRECIS,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_WANTRETURN, GWL_STYLE, GetCursorPos, GetDlgItem, GetWindowLongPtrW, GetWindowTextLengthW,
    GetWindowTextW, HICON, HMENU, ICON_BIG, ICON_SMALL, IDC_ARROW, IsIconic, IsWindowVisible,
    LoadCursorW, MSG, MoveWindow, RegisterClassW, SM_CXICON, SM_CXSMICON, SW_HIDE, SW_RESTORE,
    SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW,
    ShowWindow, WA_INACTIVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_CLOSE, WM_DPICHANGED,
    WM_GETFONT, WM_KEYDOWN, WM_SETFONT, WM_SETICON, WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE,
    WS_HSCROLL, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, w};

use windows::Win32::UI::Controls::{
    EM_EMPTYUNDOBUFFER, EM_GETSEL, EM_REPLACESEL, EM_SETLIMITTEXT, EM_SETSEL,
};

use crate::clipboard::{self, ClipText};
use crate::tray;

const CLASS_NAME: PCWSTR = w!("ClipEditEditor");
const EDIT_ID: i32 = 100;
/// 長い行のあるテキストを入れる、折り返さない編集欄（監査 S10）
const EDIT_NOWRAP_ID: i32 = 101;
const BASE_WIDTH: i32 = 800;
const BASE_HEIGHT: i32 = 500;
const FONT_POINTS: i32 = 11;
const TITLE: &str = "ClipEdit  (Ctrl+Enter: クリップボードに書き込む / Esc: 閉じる)";

thread_local! {
    // show_with が位置を決めている最中は WM_DPICHANGED の推奨位置を使わない
    static POSITIONING: Cell<bool> = const { Cell::new(false) };
    // 開いている内容に「履歴に残さない」印が付いていたか。書き込むときに印を付け直す
    static PRIVATE: Cell<bool> = const { Cell::new(false) };
}

/// 改行を CRLF にそろえる（LF だけ・CR だけの改行も EDIT コントロールで改行として見えるように）。
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// 折り返して表示すると開くのが遅くなる1行の長さ（文字数）。これより長い行があるテキストは、
/// 折り返さない編集欄で開く（監査 S10・台帳 D46）。折り返すと、空白も改行もない行は長さの2乗に近く遅くなる
/// （開くまで 1 万文字 0.5 秒、2 万文字 1.4 秒、5 万文字 6.5 秒）。この上限ちょうどでは 0.3 秒。
const WRAP_LIMIT: usize = 5_000;

/// text に、`WRAP_LIMIT` 文字より長い行があるか。
pub fn has_long_line(text: &str) -> bool {
    // バイト数は文字数以上なので、バイト数で足りる行は数えずに済ませる
    text.split(['\r', '\n'])
        .any(|line| line.len() > WRAP_LIMIT && line.chars().count() > WRAP_LIMIT)
}

/// 読み込み結果に応じたウィンドウタイトル。
pub fn title_for(content: &Result<Option<ClipText>, String>) -> String {
    match content {
        Ok(Some(_)) => TITLE.to_string(),
        Ok(None) => format!("{TITLE}  (テキストなし)"),
        Err(e) => format!("{TITLE}  - {e}"),
    }
}

/// 作業領域 work の中央に置く矩形 (x, y, 幅, 高さ)。作業領域より大きければ縮める。
pub fn center_rect(work: RECT, width: i32, height: i32) -> (i32, i32, i32, i32) {
    // 作業領域が逆向きや極端な値でもあふれないよう i64 で計算し、大きさは 0 以上にする（監査 S16）
    let span = |from: i32, to: i32| (i64::from(to) - i64::from(from)).max(0);
    let (work_w, work_h) = (span(work.left, work.right), span(work.top, work.bottom));
    let w = i64::from(width).clamp(0, work_w);
    let h = i64::from(height).clamp(0, work_h);
    let x = i64::from(work.left) + (work_w - w) / 2;
    let y = i64::from(work.top) + (work_h - h) / 2;
    (to_i32(x), to_i32(y), to_i32(w), to_i32(h))
}

/// i32 に収まるように切り詰める（`center_rect` の結果は、作業領域の中なので本来は必ず収まる）。
fn to_i32(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Newline,
    Space,
    Word,
    Hiragana,
    Katakana,
    Kanji,
    Other,
}

fn char_kind(c: char) -> CharKind {
    match c {
        '\r' | '\n' => CharKind::Newline,
        c if c.is_whitespace() => CharKind::Space,
        '\u{3041}'..='\u{309F}' => CharKind::Hiragana,
        '\u{30A0}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}' => CharKind::Katakana,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '々' => {
            CharKind::Kanji
        }
        c if c.is_alphanumeric() || c == '_' => CharKind::Word,
        _ => CharKind::Other,
    }
}

/// Ctrl+Backspace で消す範囲の先頭（UTF-16 の位置）。caret の直前の改行は改行だけ、
/// それ以外は直前の空白と、その前にある同じ種類の文字（英数字・ひらがな・カタカナ・漢字・記号）の並びを消す。
pub fn prev_word_start(units: &[u16], caret: usize) -> usize {
    let mut chars = Vec::new();
    let mut index = 0;
    for c in char::decode_utf16(units[..caret].iter().copied()) {
        let c = c.unwrap_or('\u{FFFD}');
        chars.push((index, c));
        index += c.len_utf16();
    }
    let mut i = chars.len();
    let kind_at = |i: usize| char_kind(chars[i - 1].1);
    if i > 0 && kind_at(i) == CharKind::Newline {
        i -= 1;
        if chars[i].1 == '\n' && i > 0 && chars[i - 1].1 == '\r' {
            i -= 1;
        }
    } else {
        while i > 0 && kind_at(i) == CharKind::Space {
            i -= 1;
        }
        if i > 0 && kind_at(i) != CharKind::Newline {
            let kind = kind_at(i);
            while i > 0 && kind_at(i) == kind {
                i -= 1;
            }
        }
    }
    chars.get(i).map_or(caret, |&(index, _)| index)
}

fn scale(value: i32, dpi: u32) -> i32 {
    value * dpi as i32 / 96
}

/// 編集ウィンドウ。編集欄は2つあり、どちらか一方だけを表示する（監査 S10）。
/// ふだんは折り返す `edit`、`WRAP_LIMIT` より長い行があるテキストは折り返さない `edit_nowrap`。
#[derive(Clone, Copy)]
pub struct Editor {
    hwnd: HWND,
    edit: HWND,
    edit_nowrap: HWND,
}

/// 編集欄を1つ作る。wrap なら折り返して表示し、そうでなければ横スクロールにする。
/// 折り返さないほうは隠しておき、使うときに `show_with` が表示する。
fn create_edit(
    parent: HWND,
    instance: HINSTANCE,
    id: i32,
    wrap: bool,
) -> windows::core::Result<HWND> {
    let mut style = WS_CHILD
        | WS_VSCROLL
        | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32);
    if wrap {
        style |= WS_VISIBLE;
    } else {
        style |= WS_HSCROLL | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
    }
    // SAFETY: "EDIT" は w! で作った 0 終端の文字列。parent はこのスレッドで作った自分の窓。
    // EM_SETLIMITTEXT はポインタを渡さない。
    unsafe {
        let edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            PCWSTR::null(),
            style,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(usize::try_from(id).unwrap_or_default() as *mut _)),
            Some(instance),
            None,
        )?;
        // 既定の上限は 32,767 文字。0 を渡すと上限を最大にする
        SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(0)), Some(LPARAM(0)));
        apply_font(edit, GetDpiForWindow(parent));
        Ok(edit)
    }
}

impl Editor {
    pub fn create(instance: HINSTANCE) -> windows::core::Result<Editor> {
        // SAFETY: クラス名は w! で作った 0 終端の文字列。wnd_proc は extern "system" のウィンドウ関数。
        // どのウィンドウもこのスレッドで作り、このスレッドで使う。
        unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance,
                lpszClassName: CLASS_NAME,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS_NAME,
                &HSTRING::from(TITLE),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                BASE_WIDTH,
                BASE_HEIGHT,
                None,
                None,
                Some(instance),
                None,
            )?;
            let edit = create_edit(hwnd, instance, EDIT_ID, true)?;
            let edit_nowrap = create_edit(hwnd, instance, EDIT_NOWRAP_ID, false)?;
            apply_icons(hwnd, GetDpiForWindow(hwnd));
            Ok(Editor {
                hwnd,
                edit,
                edit_nowrap,
            })
        }
    }

    /// 今表示している編集欄。
    fn active(&self) -> HWND {
        active_edit(self.edit, self.edit_nowrap)
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: 値を渡すだけ。
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    /// すでに開いていれば前面に出すだけ（編集中の内容を消さない）。閉じていれば read で読み込んで開く。
    pub fn open(&self, read: impl FnOnce() -> Result<Option<ClipText>, String>) {
        if self.is_visible() {
            self.activate();
        } else {
            self.show_with(read());
        }
    }

    pub fn show_with(&self, content: Result<Option<ClipText>, String>) {
        let mut title = title_for(&content);
        let text = match content {
            Ok(Some(clip)) => {
                PRIVATE.with(|p| p.set(clip.private));
                normalize_newlines(&clip.text)
            }
            _ => {
                PRIVATE.with(|p| p.set(false));
                String::new()
            }
        };
        // 長い行は折り返すと開くのがとても遅いので、折り返さない編集欄に入れる（監査 S10）
        let wrap = !has_long_line(&text);
        let (shown, hidden) = if wrap {
            (self.edit, self.edit_nowrap)
        } else {
            title.push_str("  (長い行があるため折り返さずに表示)");
            (self.edit_nowrap, self.edit)
        };
        // SAFETY: 文字列は HSTRING（0 終端）と w! で渡す。cursor と info はローカル変数で、info には cbSize を設定してある。
        unsafe {
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(title));
            let _ = ShowWindow(hidden, SW_HIDE);
            let _ = SetWindowTextW(hidden, w!(""));
            let _ = ShowWindow(shown, SW_SHOW);
            let _ = SetWindowTextW(shown, &HSTRING::from(text));
            self.select_all();

            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(monitor, &mut info);
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let (x, y, w, h) = center_rect(
                info.rcWork,
                scale(BASE_WIDTH, dpi_x),
                scale(BASE_HEIGHT, dpi_x),
            );

            POSITIONING.with(|p| p.set(true));
            let _ = SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOZORDER);
            POSITIONING.with(|p| p.set(false));
            let dpi = GetDpiForWindow(self.hwnd);
            apply_font(self.edit, dpi);
            apply_font(self.edit_nowrap, dpi);
        }
        self.activate();
    }

    fn activate(&self) {
        // SAFETY: 自分のウィンドウに値を渡すだけ。
        unsafe {
            // 最小化中は SW_SHOW では元に戻らない
            let command = if IsIconic(self.hwnd).as_bool() {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            let _ = ShowWindow(self.hwnd, command);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.active()));
        }
    }

    /// 隠すときは内容と「元に戻す」の履歴も消す（閉じた後に他のアプリから読まれないように）。
    pub fn hide(&self) {
        // SAFETY: 自分のウィンドウに値と w!("") を渡すだけ。EM_EMPTYUNDOBUFFER はポインタを渡さない。
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            for edit in [self.edit, self.edit_nowrap] {
                let _ = SetWindowTextW(edit, w!(""));
                SendMessageW(edit, EM_EMPTYUNDOBUFFER, None, None);
            }
        }
        PRIVATE.with(|p| p.set(false));
    }

    pub fn text(&self) -> String {
        // SAFETY: buf は長さ+1（終端の 0 の分）を確保し、GetWindowTextW はその大きさを超えて書かない。
        // 返った文字数（buf の長さ以下）までを読む。
        unsafe {
            let edit = self.active();
            let len = GetWindowTextLengthW(edit);
            let mut buf = vec![0u16; len as usize + 1];
            let copied = GetWindowTextW(edit, &mut buf);
            String::from_utf16_lossy(&buf[..copied as usize])
        }
    }

    fn select_all(&self) {
        // SAFETY: EM_SETSEL はポインタを渡さない。
        unsafe {
            SendMessageW(self.active(), EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        }
    }

    pub fn set_title(&self, title: &str) {
        // SAFETY: title を HSTRING（0 終端）にして渡す。
        unsafe {
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(title));
        }
    }

    fn commit(&self) {
        let private = PRIVATE.with(|p| p.get());
        match clipboard::write_text(Some(self.hwnd), &self.text(), private) {
            Ok(()) => self.hide(),
            Err(e) => self.set_title(&format!("{TITLE}  - 書き込み失敗: {e}")),
        }
    }

    /// メッセージループで `TranslateMessage` の前に呼ぶ。処理したキーなら true。
    /// IME で変換中の Enter / Esc は `VK_PROCESSKEY` として届くので、ここでは反応しない。
    pub fn handle_key(&self, msg: &MSG) -> bool {
        if (msg.hwnd != self.edit && msg.hwnd != self.edit_nowrap) || msg.message != WM_KEYDOWN {
            return false;
        }
        // SAFETY: 値を渡すだけ。
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        self.press(msg.wParam.0 as u16, ctrl)
    }

    fn press(&self, key: u16, ctrl: bool) -> bool {
        if key == VK_RETURN.0 && ctrl {
            self.commit();
            true
        } else if key == VK_ESCAPE.0 {
            self.hide();
            true
        } else if key == b'A' as u16 && ctrl {
            self.select_all();
            true
        } else if key == VK_BACK.0 && ctrl {
            // 標準の EDIT コントロールは Ctrl+Backspace で □ (0x7F) を入れてしまうので、自前で1語消す
            self.delete_previous_word();
            true
        } else {
            false
        }
    }

    fn delete_previous_word(&self) {
        let edit = self.active();
        // SAFETY: EM_GETSEL には、この関数のローカル変数 start と end（u32、メッセージが求める DWORD）の場所を渡す。
        // EM_SETSEL の範囲は caret が文字列の長さ以下に切り詰めてあり、from はその手前。
        // EM_REPLACESEL の文字列は w! で作った 0 終端の定数。
        unsafe {
            let (mut start, mut end) = (0u32, 0u32);
            SendMessageW(
                edit,
                EM_GETSEL,
                Some(WPARAM(&mut start as *mut u32 as usize)),
                Some(LPARAM(&mut end as *mut u32 as isize)),
            );
            if start == end {
                let units: Vec<u16> = self.text().encode_utf16().collect();
                let caret = (end as usize).min(units.len());
                let from = prev_word_start(&units, caret);
                SendMessageW(
                    edit,
                    EM_SETSEL,
                    Some(WPARAM(from)),
                    Some(LPARAM(caret as isize)),
                );
            }
            // WPARAM(1): 元に戻す (Ctrl+Z) で戻せるようにする
            SendMessageW(
                edit,
                EM_REPLACESEL,
                Some(WPARAM(1)),
                Some(LPARAM(w!("").as_ptr() as isize)),
            );
        }
    }
}

fn apply_font(edit: HWND, dpi: u32) {
    // SAFETY: フォント名は w! で作った 0 終端の文字列。WM_SETFONT の後は編集欄が新しいフォントを使うので、
    // 前のフォントを破棄する。ふだんは自分で作ったもの（最初はシステムのフォントで 0 が返る）。
    // ほかのプロセスが WM_SETFONT で自分のハンドルを送り込んでいた場合は、そのプロセスのハンドルなので
    // DeleteObject が失敗するだけで、こちらのフォントが1つ漏れる（監査 S15）。
    unsafe {
        let old = SendMessageW(edit, WM_GETFONT, None, None);
        let font = CreateFontW(
            -(FONT_POINTS * dpi as i32 / 72),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (FIXED_PITCH.0 | FF_MODERN.0) as u32,
            w!("BIZ UDゴシック"),
        );
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        if old.0 != 0 {
            let _ = DeleteObject(HGDIOBJ(HFONT(old.0 as *mut _).0));
        }
    }
}

/// タイトルバーとタスクバーのアイコンを、トレイと同じアイコンから dpi に合った大きさで作って付ける。
fn apply_icons(hwnd: HWND, dpi: u32) {
    for (kind, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
        // SAFETY: 値だけを渡す。
        let size = unsafe { GetSystemMetricsForDpi(metric, dpi) };
        let Ok(icon) = tray::load_app_icon(size) else {
            continue;
        };
        // SAFETY: icon は今作った自分のアイコン。WM_SETICON は前のアイコンを返すので、それを破棄する。
        // ふだんは自分が前に付けたもの（最初は 0）。ほかのプロセスが WM_SETICON で送り込んだものなら、
        // DestroyIcon が失敗するだけで、こちらのアイコンが1つ漏れる（監査 S15）。
        unsafe {
            let old = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(kind as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
            if old.0 != 0 {
                let _ = DestroyIcon(HICON(old.0 as *mut _));
            }
        }
    }
}

/// 編集ウィンドウ hwnd と、その中の2つの編集欄。
fn editor_of(hwnd: HWND) -> Option<Editor> {
    // SAFETY: 値を渡すだけ。見つからなければ Err が返る。
    unsafe {
        Some(Editor {
            hwnd,
            edit: GetDlgItem(Some(hwnd), EDIT_ID).ok()?,
            edit_nowrap: GetDlgItem(Some(hwnd), EDIT_NOWRAP_ID).ok()?,
        })
    }
}

/// 2つの編集欄のうち、表示する側（`WS_VISIBLE` の付いているほう）。
/// `IsWindowVisible` は親が隠れていると false を返すので、開く途中でも正しく選べるよう、自分の印を見る。
fn active_edit(edit: HWND, edit_nowrap: HWND) -> HWND {
    // SAFETY: 値を渡すだけ。
    let style = unsafe { GetWindowLongPtrW(edit_nowrap, GWL_STYLE) };
    if style & isize::try_from(WS_VISIBLE.0).unwrap_or_default() != 0 {
        edit_nowrap
    } else {
        edit
    }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: hwnd・lparam・wparam はシステムから受け取ったもの。WM_DPICHANGED の lparam は、
    // システムが渡す推奨位置の RECT を指す（Windows の仕様）。それ以外はそのまま DefWindowProcW に渡す。
    // ほかのプロセスからは、PostMessage では WM_DPICHANGED を送れず（ERROR_MESSAGE_SYNC_ONLY）、
    // SendMessage でも送り手のポインタが不正なら届かない（監査 S15 で確かめた）。
    unsafe {
        match msg {
            WM_SIZE => {
                let width = (lparam.0 & 0xFFFF) as i32;
                let height = ((lparam.0 >> 16) & 0xFFFF) as i32;
                if let Some(editor) = editor_of(hwnd) {
                    for edit in [editor.edit, editor.edit_nowrap] {
                        let _ = MoveWindow(edit, 0, 0, width, height, true);
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                if let Some(editor) = editor_of(hwnd) {
                    editor.hide();
                } else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                if (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE
                    && let Some(editor) = editor_of(hwnd)
                {
                    let _ = SetFocus(Some(editor.active()));
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                if !POSITIONING.with(|p| p.get()) {
                    let r = &*(lparam.0 as *const RECT);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER,
                    );
                }
                let dpi = (wparam.0 & 0xFFFF) as u32;
                if let Some(editor) = editor_of(hwnd) {
                    apply_font(editor.edit, dpi);
                    apply_font(editor.edit_nowrap, dpi);
                }
                apply_icons(hwnd, dpi);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    #[test]
    fn normalizes_all_newline_kinds_to_crlf() {
        assert_eq!(normalize_newlines("a\nb"), "a\r\nb");
        assert_eq!(normalize_newlines("a\rb"), "a\r\nb");
        assert_eq!(normalize_newlines("a\r\nb"), "a\r\nb");
        assert_eq!(normalize_newlines("a\n\r\nb\r"), "a\r\n\r\nb\r\n");
        assert_eq!(normalize_newlines(""), "");
    }

    // 監査 S10: 折り返すと開くのが遅くなるほど長い行があるときだけ、折り返さない編集欄で開く
    #[test]
    fn long_line_is_detected_by_chars_not_bytes() {
        assert!(!has_long_line(""));
        assert!(
            !has_long_line(&"x".repeat(WRAP_LIMIT)),
            "ちょうど上限は折り返す"
        );
        assert!(has_long_line(&"x".repeat(WRAP_LIMIT + 1)));
        assert!(
            !has_long_line(&"あ".repeat(WRAP_LIMIT)),
            "バイト数ではなく文字数で数える"
        );
        let many_short = format!("{}\r\n", "y".repeat(100)).repeat(WRAP_LIMIT);
        assert!(
            !has_long_line(&many_short),
            "全体が長くても、行が短ければ折り返す"
        );
        let one_long = format!("短い行\r\n{}\nまた短い行", "z".repeat(WRAP_LIMIT + 1));
        assert!(has_long_line(&one_long), "途中の1行だけ長い");
    }

    #[test]
    fn title_reflects_content() {
        assert_eq!(title_for(&plain("x")), TITLE);
        assert!(title_for(&Ok(None)).ends_with("(テキストなし)"));
        assert!(title_for(&Err("開けません".into())).ends_with("開けません"));
    }

    #[test]
    fn centers_inside_work_area() {
        let work = RECT {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1040,
        };
        assert_eq!(center_rect(work, 800, 500), (2480, 270, 800, 500));
    }

    #[test]
    fn shrinks_to_small_work_area() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 640,
            bottom: 400,
        };
        assert_eq!(center_rect(work, 1200, 750), (0, 0, 640, 400));
    }

    // 監査 S16: 作業領域が極端な値・逆向きでも、あふれてパニックせず、大きさは 0 以上になる
    #[test]
    fn extreme_work_area_does_not_overflow() {
        let work = RECT {
            left: 1_167_600_539,
            top: i32::MIN,
            right: -979_883_110,
            bottom: i32::MAX,
        };
        let (_, _, w, h) = center_rect(work, 800, 500);
        assert!(w >= 0 && h >= 0, "幅 {w}・高さ {h}");
        let inverted = RECT {
            left: 100,
            top: 0,
            right: 0,
            bottom: 50,
        };
        assert_eq!(center_rect(inverted, 800, 500), (100, 0, 0, 50));
    }

    fn word_start(text: &str) -> usize {
        let units: Vec<u16> = text.encode_utf16().collect();
        prev_word_start(&units, units.len())
    }

    #[test]
    fn prev_word_start_handles_ascii_words_and_spaces() {
        assert_eq!(word_start("hello world"), 6);
        assert_eq!(word_start("hello world  "), 6, "直前の空白も一緒に消す");
        assert_eq!(word_start("snake_case1"), 0);
        assert_eq!(word_start("a, b."), 4, "記号は記号だけで1語");
        assert_eq!(word_start(""), 0);
    }

    #[test]
    fn prev_word_start_splits_japanese_by_script() {
        // 「日本語のテキスト」→ テキスト / の / 日本語 の順に消える
        assert_eq!(word_start("日本語のテキスト"), 4);
        assert_eq!(word_start("日本語の"), 3);
        assert_eq!(word_start("日本語"), 0);
        assert_eq!(word_start("コーヒー"), 0, "長音記号はカタカナの一部");
    }

    #[test]
    fn prev_word_start_deletes_line_break_as_one_unit() {
        assert_eq!(word_start("abc\r\n"), 3);
        assert_eq!(word_start("abc\n"), 3);
        assert_eq!(word_start("abc\r\ndef"), 5, "改行の手前までは戻らない");
    }

    #[test]
    fn prev_word_start_counts_surrogate_pairs_as_one_char() {
        assert_eq!(word_start("ab😀😀"), 2, "絵文字は記号の扱い");
    }

    #[test]
    fn prev_word_start_uses_caret_position() {
        let units: Vec<u16> = "foo bar baz".encode_utf16().collect();
        assert_eq!(prev_word_start(&units, 7), 4);
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
        assert!(editor.press(VK_BACK.0, true));
        assert_eq!(editor.text(), "hello 日本語の");
        assert!(editor.press(VK_BACK.0, true));
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
        assert!(editor.press(VK_BACK.0, true));
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
            assert!(editor.press(VK_RETURN.0, true));
            let written = clipboard::read_text().unwrap().unwrap();
            assert_eq!(written.text, "P@ssw0rd");
            assert_eq!(written.private, private);
        }
    }

    #[test]
    #[ignore = "実際にウィンドウを出す"]
    fn plain_backspace_is_left_to_edit_control() {
        let editor = new_editor();
        editor.show_with(plain("abc"));
        assert!(!editor.press(VK_BACK.0, false));
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
            assert_eq!(tray::icon_width(HICON(icon.0 as *mut _)), expected);
        }
    }
}
