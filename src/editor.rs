use std::cell::{Cell, RefCell};
use std::path::Path;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_NUMPAD1, VK_NUMPAD9,
    VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN,
    GW_CHILD, GW_HWNDNEXT, GWL_STYLE, GetCursorPos, GetWindow, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsIconic, IsWindowVisible, LoadCursorW,
    MSG, MoveWindow, RegisterClassW, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, WA_INACTIVE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_ACTIVATE, WM_CLOSE, WM_COMMAND, WM_DPICHANGED, WM_INITMENUPOPUP, WM_KEYDOWN,
    WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_HSCROLL, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, w};

use windows::Win32::UI::Controls::{
    EM_EMPTYUNDOBUFFER, EM_GETSEL, EM_REPLACESEL, EM_SCROLLCARET, EM_SETLIMITTEXT, EM_SETSEL,
};

use crate::clipboard::{self, ClipText};
use crate::favorites::{self, Favorite};
use crate::textops::{has_long_line, normalize_newlines, prev_word_start};
use crate::tray::{self, NoticeKind};

mod copy;
mod dpi;
mod menu;
mod title;

pub use title::title_for;
use title::{favorite_title, note_long_line, show_failure};

const CLASS_NAME: PCWSTR = w!("ClipEditEditor");
// お気に入りのコマンド ID（100〜121）と重ならないようにする（監査 S28）
const EDIT_ID: i32 = 1000;
const _: () = assert!(crate::favorite_menu::CMD_ORGANIZE < EDIT_ID as usize);
/// 長い行のあるテキストを入れる、折り返さない編集欄（監査 S10）
const EDIT_NOWRAP_ID: i32 = 1001;
const BASE_WIDTH: i32 = 800;
const BASE_HEIGHT: i32 = 500;
const TITLE: &str = "ClipEdit  (Ctrl+Enter: クリップボードに書き込む / Esc: 閉じる)";

thread_local! {
    // show_with が位置を決めている最中は WM_DPICHANGED の推奨位置を使わない
    static POSITIONING: Cell<bool> = const { Cell::new(false) };
    // 開いている内容に「履歴に残さない」印が付いていたか。書き込むときに印を付け直す
    static PRIVATE: Cell<bool> = const { Cell::new(false) };
    // お気に入りの追加・整理の結果を知らせるトレイのウィンドウ。None のときは通知を出さない
    static NOTICE_WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    // 作った編集ウィンドウ。wnd_proc はここから自分の Editor を引く。窓のデータ（GWLP_USERDATA）や
    // 編集欄の ID は、同じユーザーのほかのプロセスが書き換えられるので頼らない（監査 S24・S30）
    static EDITORS: RefCell<Vec<Editor>> = const { RefCell::new(Vec::new()) };
}

/// お気に入りのメニューから選んだ操作の結果を、hwnd のトレイアイコンから通知で知らせるようにする。
pub fn set_notice_window(hwnd: HWND) {
    NOTICE_WINDOW.with(|w| w.set(Some(hwnd)));
}

fn notice_window() -> Option<HWND> {
    NOTICE_WINDOW.with(std::cell::Cell::get)
}

fn notice(kind: NoticeKind, title: &str, message: &str) {
    if let Some(hwnd) = notice_window() {
        tray::show_notice(hwnd, kind, title, message);
    }
}

/// 編集欄で押されたキーに対応する操作。
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    Commit,
    Hide,
    SelectAll,
    DeleteWord,
    /// お気に入りの番号（0 から）
    Recall(usize),
}

/// IME で変換中のキーは `VK_PROCESSKEY` として届くので、どれにも当たらない。
/// `AltGr` は Ctrl+Alt として届くので、Alt が押されているときはお気に入りの呼び出しにせず、
/// 文字の入力に回す（ドイツ語などの配列で AltGr+7 が `{` になるため）。
fn key_action(key: u16, ctrl: bool, alt: bool) -> Option<KeyAction> {
    if !ctrl {
        return (key == VK_ESCAPE.0).then_some(KeyAction::Hide);
    }
    let digit = u16::from(b'1')..=u16::from(b'9');
    let numpad = VK_NUMPAD1.0..=VK_NUMPAD9.0;
    match key {
        k if k == VK_RETURN.0 => Some(KeyAction::Commit),
        k if k == VK_ESCAPE.0 => Some(KeyAction::Hide),
        k if k == u16::from(b'A') => Some(KeyAction::SelectAll),
        k if k == VK_BACK.0 => Some(KeyAction::DeleteWord),
        _ if alt => None,
        k if digit.contains(&k) => Some(KeyAction::Recall(usize::from(k - digit.start()))),
        k if numpad.contains(&k) => Some(KeyAction::Recall(usize::from(k - numpad.start()))),
        _ => None,
    }
}

/// `WM_KEYDOWN` の lParam が、押しっぱなしによる繰り返し（bit 30: 直前も押されていた）か。
fn is_repeat(lparam: isize) -> bool {
    lparam & (1 << 30) != 0
}

/// 編集欄の内容を消す。
fn forget(edit: HWND) {
    set_text(edit, "");
    empty_undo(edit);
}

/// ウィンドウ（編集欄など）の文字列を text にする。
fn set_text(hwnd: HWND, text: &str) {
    // SAFETY: text を HSTRING（0 終端）にして渡す。
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(text));
    }
}

/// ウィンドウ（編集欄など）の文字列。
fn window_text(hwnd: HWND) -> String {
    // SAFETY: buf は長さ+1（終端の 0 の分）を確保し、GetWindowTextW はその大きさを超えて書かない。
    // 返った文字数（buf の長さ以下）までを読む。
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; usize::try_from(len).unwrap_or_default() + 1];
        let copied = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..usize::try_from(copied).unwrap_or_default()])
    }
}

/// 編集欄で選んでいる範囲（UTF-16 の位置で、始まり, 終わり）。何も選んでいなければ両方がキャレットの位置。
fn selection(edit: HWND) -> (usize, usize) {
    let (mut start, mut end) = (0u32, 0u32);
    // SAFETY: EM_GETSEL には、この関数のローカル変数 start と end（u32、メッセージが求める DWORD）の場所を渡す。
    unsafe {
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&raw mut start as usize)),
            Some(LPARAM(&raw mut end as isize)),
        );
    }
    (start as usize, end as usize)
}

/// 編集欄の start から end まで（UTF-16 の位置）を選ぶ。同じ位置ならキャレットを置くだけ。
/// end が本文より後ろなら末尾まで（`usize::MAX` で全文）。
fn set_selection(edit: HWND, start: usize, end: usize) {
    // EM_SETSEL は位置を int として読み、-1 を「末尾まで」として扱う
    let end = i32::try_from(end).unwrap_or(-1);
    // SAFETY: EM_SETSEL はポインタを渡さない。範囲が文字列の外なら、編集欄が切り詰める。
    unsafe {
        SendMessageW(
            edit,
            EM_SETSEL,
            Some(WPARAM(start)),
            Some(LPARAM(isize::try_from(end).unwrap_or(-1))),
        );
    }
}

/// 編集欄で選んでいるところを text に置き換える。元に戻す (Ctrl+Z) で戻せる。
fn replace_selection(edit: HWND, text: &str) {
    let text = HSTRING::from(text);
    // SAFETY: text は HSTRING（0 終端）で、呼び出しの間は生きている。
    // WPARAM(1): 元に戻す (Ctrl+Z) で戻せるようにする
    unsafe {
        SendMessageW(
            edit,
            EM_REPLACESEL,
            Some(WPARAM(1)),
            Some(LPARAM(text.as_ptr() as isize)),
        );
    }
}

/// 編集欄の「元に戻す」の履歴だけを消す。
fn empty_undo(edit: HWND) {
    // SAFETY: EM_EMPTYUNDOBUFFER はポインタを渡さない。
    unsafe {
        SendMessageW(edit, EM_EMPTYUNDOBUFFER, None, None);
    }
}

/// 表示したあとの選択のしかた。
#[derive(Clone, Copy)]
enum Select {
    /// 全部選ぶ
    All,
    /// 何も選ばず、キャレットを先頭に置く
    CaretAtStart,
}

/// 編集ウィンドウ。編集欄は2つあり、どちらか一方だけを表示する（監査 S10）。
/// ふだんは折り返す `edit`、`WRAP_LIMIT` より長い行があるテキストは折り返さない `edit_nowrap`。
#[derive(Clone, Copy)]
pub struct Editor {
    hwnd: HWND,
    edit: HWND,
    edit_nowrap: HWND,
    /// メニューバーの「お気に入り」の中身（開くたびに作り直す）
    bar_menu: HMENU,
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
        dpi::apply_font(edit, GetDpiForWindow(parent));
        copy::attach(edit)?;
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
            RegisterClassW(&raw const class);
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
            // メニューバーの「お気に入り」。中身は開く直前（WM_INITMENUPOPUP）に作る
            let bar_menu = menu::create_bar(hwnd)?;
            dpi::apply_icons(hwnd, GetDpiForWindow(hwnd));
            let editor = Editor {
                hwnd,
                edit,
                edit_nowrap,
                bar_menu,
            };
            EDITORS.with(|e| e.borrow_mut().push(editor));
            Ok(editor)
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

    /// 本文に合う編集欄（長い行があれば折り返さないほう）に text を入れて表示し、もう一方は空にして隠す。
    /// long は text に長い行があるか（呼ぶ側が1回だけ調べる）。
    fn fill(&self, text: &str, title: String, long: bool) {
        // 長い行は折り返すと開くのがとても遅いので、折り返さない編集欄に入れる（監査 S10）
        let (shown, hidden) = (self.edit_for(long), self.edit_for(!long));
        let title = note_long_line(title, long);
        self.set_title(&title);
        // SAFETY: 自分の窓に値を渡すだけ。
        unsafe {
            let _ = ShowWindow(hidden, SW_HIDE);
            let _ = ShowWindow(shown, SW_SHOW);
        }
        set_text(hidden, "");
        set_text(shown, text);
    }

    /// long なら折り返さない編集欄、そうでなければ折り返す編集欄。
    fn edit_for(&self, long: bool) -> HWND {
        if long { self.edit_nowrap } else { self.edit }
    }

    /// 2つの編集欄に、dpi に合うフォントを付ける。
    fn apply_fonts(&self, dpi: u32) {
        dpi::apply_font(self.edit, dpi);
        dpi::apply_font(self.edit_nowrap, dpi);
    }

    pub fn show_with(&self, content: Result<Option<ClipText>, String>) {
        let title = title_for(&content);
        let (text, private) = match content {
            Ok(Some(clip)) => (normalize_newlines(&clip.text), clip.private),
            _ => (String::new(), false),
        };
        self.present(&text, private, title, Select::All);
    }

    /// text を title つきで編集欄に入れ、select のとおりに選び、カーソルのあるモニターに出して前面に出す。
    fn present(&self, text: &str, private: bool, title: String, select: Select) {
        PRIVATE.with(|p| p.set(private));
        self.fill(text, title, has_long_line(text));
        // 選択はここで決める。activate の SetFocus は選択範囲を変えないので、順番はこのままでよい
        match select {
            Select::All => self.select_all(),
            Select::CaretAtStart => self.put_caret_at_start(),
        }
        // SAFETY: cursor と info はローカル変数で、info には cbSize を設定してある。
        unsafe {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&raw mut cursor);
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(monitor, &raw mut info);
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y);
            let (x, y, w, h) = dpi::center_rect(
                info.rcWork,
                dpi::scale(BASE_WIDTH, dpi_x),
                dpi::scale(BASE_HEIGHT, dpi_x),
            );

            POSITIONING.with(|p| p.set(true));
            let _ = SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOZORDER);
            POSITIONING.with(|p| p.set(false));
            let dpi = GetDpiForWindow(self.hwnd);
            self.apply_fonts(dpi);
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
        // SAFETY: 自分のウィンドウに値を渡すだけ。
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        for edit in [self.edit, self.edit_nowrap] {
            forget(edit);
        }
        PRIVATE.with(|p| p.set(false));
    }

    pub fn text(&self) -> String {
        window_text(self.active())
    }

    fn select_all(&self) {
        set_selection(self.active(), 0, usize::MAX);
    }

    /// いつものタイトルを付ける。失敗の表示（`show_failure`）はこの後ろに足す。
    pub fn set_title(&self, title: &str) {
        title::set(self.hwnd, title);
    }

    fn commit(&self) {
        let private = PRIVATE.with(std::cell::Cell::get);
        match clipboard::write_text(Some(self.hwnd), &self.text(), private) {
            Ok(()) => self.hide(),
            Err(e) => show_failure(self.hwnd, "書き込み失敗", &e),
        }
    }

    /// メッセージループで `TranslateMessage` の前に呼ぶ。処理したキーなら true。
    /// IME で変換中の Enter / Esc は `VK_PROCESSKEY` として届くので、ここでは反応しない。
    pub fn handle_key(&self, msg: &MSG) -> bool {
        if (msg.hwnd != self.edit && msg.hwnd != self.edit_nowrap) || msg.message != WM_KEYDOWN {
            return false;
        }
        // SAFETY: 値を渡すだけ。
        let (ctrl, alt) = unsafe {
            (
                GetKeyState(i32::from(VK_CONTROL.0)) < 0,
                GetKeyState(i32::from(VK_MENU.0)) < 0,
            )
        };
        self.press(msg.wParam.0 as u16, ctrl, alt, is_repeat(msg.lParam.0))
    }

    fn press(&self, key: u16, ctrl: bool, alt: bool, repeat: bool) -> bool {
        let Some(action) = key_action(key, ctrl, alt) else {
            return false;
        };
        // 押しっぱなしで入れ替えを繰り返すと、元の内容が元に戻す履歴から消えるので、2回目以降は何もしない
        if repeat && matches!(action, KeyAction::Recall(_)) {
            return true;
        }
        match action {
            KeyAction::Commit => self.commit(),
            KeyAction::Hide => self.hide(),
            KeyAction::SelectAll => self.select_all(),
            // 標準の EDIT コントロールは Ctrl+Backspace で □ (0x7F) を入れてしまうので、自前で1語消す
            KeyAction::DeleteWord => self.delete_previous_word(),
            // 番号のお気に入りがなくてもキーは使ったものとする（数字を入力しない）
            KeyAction::Recall(index) => {
                self.recall(&favorites::default_dir(), index);
            }
        }
        true
    }

    /// 閉じていればクリップボードを読まずに favorite の本文で開き、開いていれば入れ替えて前面に出す。
    pub fn open_favorite(&self, favorite: &Favorite) {
        if self.is_visible() {
            self.replace_with(favorite);
            self.activate();
        } else {
            self.present(
                &normalize_newlines(&favorite.text),
                false,
                favorite_title(&favorite.name),
                Select::CaretAtStart,
            );
        }
    }

    /// dir の index 番目（0 から）のお気に入りに入れ替える。その番号がなければ何もせず false。
    fn recall(&self, dir: &Path, index: usize) -> bool {
        match favorites::list(dir).items.get(index) {
            Some(favorite) => {
                self.replace_with(favorite);
                true
            }
            None => false,
        }
    }

    /// 編集欄の内容を favorite の本文に入れ替える。同じ編集欄のままなら Ctrl+Z で元に戻せる。
    fn replace_with(&self, favorite: &Favorite) {
        let was_private = PRIVATE.with(std::cell::Cell::get);
        let title = favorite_title(&favorite.name);
        let long = has_long_line(&favorite.text);
        let target = self.edit_for(long);
        if target == self.active() {
            // すでに同じ内容なら入れ替えない（入れ替えると、元に戻す履歴の1回分が同じ内容で上書きされる）
            if self.text() != favorite.text {
                self.select_all();
                replace_selection(target, &favorite.text);
            }
            self.set_title(&note_long_line(title, long));
        } else {
            self.fill(&favorite.text, title, long);
        }
        if was_private {
            // 戻せると、パスワードが「残さない」印なしで編集欄に戻ってしまう
            for edit in [self.edit, self.edit_nowrap] {
                empty_undo(edit);
            }
        }
        PRIVATE.with(|p| p.set(false));
        self.put_caret_at_start();
    }

    /// 何も選択せず、キャレットを本文の先頭に置いて見える位置にする。
    fn put_caret_at_start(&self) {
        let edit = self.active();
        set_selection(edit, 0, 0);
        // SAFETY: EM_SCROLLCARET はポインタを渡さない。
        unsafe {
            SendMessageW(edit, EM_SCROLLCARET, None, None);
        }
    }

    fn delete_previous_word(&self) {
        let edit = self.active();
        let (start, end) = selection(edit);
        if start == end {
            let units: Vec<u16> = self.text().encode_utf16().collect();
            let caret = end.min(units.len());
            set_selection(edit, prev_word_start(&units, caret), caret);
        }
        replace_selection(edit, "");
    }
}

/// 編集ウィンドウ hwnd と、その中の2つの編集欄。
/// `Editor::create` が作り終える前（作っている途中のメッセージ）は None。
fn editor_of(hwnd: HWND) -> Option<Editor> {
    EDITORS.with(|e| e.borrow().iter().find(|e| e.hwnd == hwnd).copied())
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
            WM_INITMENUPOPUP => {
                if let Some(editor) = editor_of(hwnd)
                    && wparam.0 == editor.bar_menu.0 as usize
                {
                    editor.refresh_menu_bar(&favorites::default_dir());
                }
                LRESULT(0)
            }
            // lparam が 0 ならメニューから（編集欄からの通知は lparam が編集欄）
            WM_COMMAND if lparam.0 == 0 => {
                if let Some(editor) = editor_of(hwnd) {
                    editor.menu_bar_command(&favorites::default_dir(), wparam.0 & 0xFFFF);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                if let Some(editor) = editor_of(hwnd) {
                    editor.hide();
                } else {
                    // 作っている途中で editor_of が取れなくても、子の窓（編集欄）はすべて空にする。
                    // ID は書き換えられるので、ID では探さない（監査 S24・S30）
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    let mut child = GetWindow(hwnd, GW_CHILD).ok();
                    while let Some(edit) = child {
                        forget(edit);
                        child = GetWindow(edit, GW_HWNDNEXT).ok();
                    }
                    PRIVATE.with(|p| p.set(false));
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
                if !POSITIONING.with(std::cell::Cell::get) {
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
                    editor.apply_fonts(dpi);
                }
                dpi::apply_icons(hwnd, dpi);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests;
