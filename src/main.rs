#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// unsafe ブロックにはなぜ安全かを書く（cargo clippy で確かめる）
#![warn(clippy::undocumented_unsafe_blocks)]

mod autostart;
mod clipboard;
mod config;
mod editor;
mod hotkey;
mod instance;
mod tray;
mod wide;

use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    WPARAM,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MB_ICONERROR,
    MB_OK, MSG, MessageBoxW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_DESTROY, WM_HOTKEY,
    WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR, w};

use editor::Editor;

const MAIN_CLASS: PCWSTR = w!("ClipEditMain");

thread_local! {
    static EDITOR: Cell<Option<Editor>> = const { Cell::new(None) };
    static ACTIVE_HOTKEY: RefCell<Option<hotkey::Active>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static TRAY_TIP: RefCell<String> = RefCell::new(String::from("ClipEdit"));
    static PENDING_NOTICES: RefCell<tray::PendingNotices> = RefCell::new(tray::PendingNotices::default());
}

fn main() {
    // ほかの DLL が読み込まれる前に行う
    restrict_dll_search();
    let Some(mutex) = single_instance() else {
        // すでに動いているほうに、編集ウィンドウを開かせて終わる（台帳 D1・D41）
        instance::hand_over_to_running(|| instance::forward_to_running(MAIN_CLASS), show_error);
        return;
    };
    // SAFETY: 定数を渡すだけ。ウィンドウを作る前に呼ぶ。失敗しても既定の DPI 設定で動く。
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // SAFETY: None は自分の exe を指し、返るハンドルは解放不要。
    let instance = unsafe { GetModuleHandleW(None) }
        .unwrap_or_else(|e| fail_startup("自分のモジュールを取得できませんでした", &e))
        .into();
    let editor = Editor::create(instance)
        .unwrap_or_else(|e| fail_startup("編集ウィンドウを作れませんでした", &e));
    EDITOR.with(|e| e.set(Some(editor)));
    let main = create_main_window(instance)
        .unwrap_or_else(|e| fail_startup("本体ウィンドウを作れませんでした", &e));
    start_tray_and_hotkey(main);
    run_message_loop(editor);
    // SAFETY: mutex は single_instance が作った自分のハンドルで、このあと使わない。
    unsafe {
        let _ = CloseHandle(mutex);
    }
}

/// トレイのアイコンとホットキーを受け取る、表示しない本体ウィンドウを作る。
fn create_main_window(instance: HINSTANCE) -> windows::core::Result<HWND> {
    // SAFETY: クラス名などの文字列は w! で作った 0 終端の定数。main_proc は extern "system" のウィンドウ関数。
    // ウィンドウはこのスレッドで作り、このスレッドのメッセージループで処理する。
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: instance,
            lpszClassName: MAIN_CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
        // HWND_MESSAGE の窓は TaskbarCreated を受け取れないので、表示しない普通のウィンドウにする
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            MAIN_CLASS,
            w!("ClipEdit"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
    }
}

/// 設定を読んでホットキーを登録し、トレイにアイコンを出して、必要なら通知する。
fn start_tray_and_hotkey(main: HWND) {
    // ツールチップに登録できたかどうかを出すため、トレイに出す前に登録する
    let applied = apply_config(main);
    add_tray_icon(main);
    if let Some(warning) = &applied.warning {
        show_notice(
            main,
            tray::NoticeKind::Warning,
            "ClipEdit: 設定を既定値で起動しました",
            warning,
        );
    }
    if !applied.outcome.is_registered() {
        show_notice(
            main,
            tray::NoticeKind::Warning,
            "ClipEdit",
            &hotkey_summary(&applied.outcome, None),
        );
    }
}

/// 終了するまでメッセージを処理する。編集ウィンドウのキーは `TranslateMessage` の前に見る。
fn run_message_loop(editor: Editor) {
    let mut msg = MSG::default();
    // SAFETY: msg はローカル変数で、GetMessageW が書き込んだものだけを使う。
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if editor.handle_key(&msg) {
            continue;
        }
        // SAFETY: msg は GetMessageW が今受け取ったメッセージ。
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 設定ファイルを読んでホットキーを反映した結果。
struct AppliedConfig {
    outcome: hotkey::Outcome,
    warning: Option<String>,
}

/// 設定ファイルを読み、ホットキーを切り替えて、ツールチップの文字を決める。
fn apply_config(hwnd: HWND) -> AppliedConfig {
    let (config, config_warning) = config::load(&config::default_path());
    let (key, text, hotkey_warning) = choose_hotkey(&config.hotkey);
    let warning = config_warning.or(hotkey_warning);
    let outcome = ACTIVE_HOTKEY.with(|a| {
        let mut active = a.borrow_mut();
        let (next, outcome) = hotkey::switch(
            active.take(),
            key,
            &text,
            warning.is_some(),
            |id, key| hotkey::register(hwnd, id, key).is_ok(),
            |id| hotkey::unregister(hwnd, id),
        );
        TRAY_TIP.with(|t| *t.borrow_mut() = tray_tip(next.as_ref()));
        *active = next;
        outcome
    });
    AppliedConfig { outcome, warning }
}

/// トレイの「設定を読み直す」。起動し直さずに config.toml を反映する（台帳 D9）。
fn reload_config(hwnd: HWND) {
    let applied = apply_config(hwnd);
    TRAY_TIP.with(|t| tray::set_tip(hwnd, &t.borrow()));
    show_notice(
        hwnd,
        reload_notice_kind(applied.outcome.is_registered(), applied.warning.as_deref()),
        "ClipEdit: 設定を読み直しました",
        &hotkey_summary(&applied.outcome, applied.warning.as_deref()),
    );
}

/// 読み直した結果の通知の種類。警告がなく、ホットキーを登録できたときだけ情報にする（台帳 D34）。
fn reload_notice_kind(registered: bool, warning: Option<&str>) -> tray::NoticeKind {
    if registered && warning.is_none() {
        tray::NoticeKind::Info
    } else {
        tray::NoticeKind::Warning
    }
}

/// 設定を読んだ結果の知らせ。警告があれば先に書く。
fn hotkey_summary(outcome: &hotkey::Outcome, warning: Option<&str>) -> String {
    let status = match outcome {
        hotkey::Outcome::Registered(text) => format!("ホットキーは {text} です"),
        hotkey::Outcome::Kept(text) => format!("ホットキーは {text} のままです"),
        hotkey::Outcome::Failed {
            wanted,
            kept: Some(kept),
        } => format!("ホットキー {wanted} を登録できませんでした。{kept} のまま使えます。"),
        hotkey::Outcome::Failed { wanted, kept: None } => {
            format!("ホットキー {wanted} を登録できませんでした。トレイアイコンから開けます。")
        }
    };
    match warning {
        Some(warning) => format!(
            "{warning}
{status}"
        ),
        None => status,
    }
}

/// トレイにアイコンを出す。出せたら、それまでに出せなかった通知を送り直す（台帳 D3）。
fn add_tray_icon(hwnd: HWND) {
    if TRAY_TIP.with(|t| tray::add(hwnd, &t.borrow())).is_ok() {
        PENDING_NOTICES.with(|p| {
            p.borrow_mut().retry(|k, t, m| tray::notify(hwnd, k, t, m));
        });
    }
}

/// 通知を出す。トレイの準備ができていなくて出せなければ、覚えておいてあとで出す。
fn show_notice(hwnd: HWND, kind: tray::NoticeKind, title: &str, message: &str) {
    PENDING_NOTICES.with(|p| {
        p.borrow_mut()
            .post(kind, title, message, |k, t, m| tray::notify(hwnd, k, t, m));
    });
}

/// 起動できなかった理由をメッセージボックスで出して終了する（台帳 D2）。
/// `windows_subsystem = "windows"` ではパニックしても画面に何も出ないため。
fn fail_startup(what: &str, error: &windows::core::Error) -> ! {
    show_error(&startup_error_text(what, &error.message()));
    std::process::exit(1);
}

/// 名前だけで読み込まれる DLL を、System32 からだけ探すようにする（監査 S9）。
/// 既定では exe と同じフォルダを先に探すので、そこに置かれた偽の DLL を、システムの DLL が
/// あとから読み込んでしまう（`WindowsCodecs.dll`・`CRYPTBASE.DLL` など）。
fn restrict_dll_search() {
    // SAFETY: 定数を渡すだけ。失敗しても既定の探し方のまま動く。
    unsafe {
        let _ = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
    }
}

/// エラーをメッセージボックスで出す（トレイの準備がない起動の途中で使う）。
fn show_error(text: &str) {
    // SAFETY: 文字列は HSTRING と w! で作った 0 終端の文字列。親ウィンドウは持たない。
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            w!("ClipEdit"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn startup_error_text(what: &str, detail: &str) -> String {
    format!(
        "ClipEdit を起動できませんでした。
{what}: {detail}"
    )
}

/// 設定に書かれたホットキーから、使うホットキー・表示用の文字列・警告を決める。
/// 書き方が正しくなければ既定のホットキーにし、理由を警告で返す。
/// 表示用の文字列は設定の書き方ではなく、決まった書き方にする（台帳 D42）。
fn choose_hotkey(text: &str) -> (hotkey::HotKey, String, Option<String>) {
    let (hk, warning) = match hotkey::parse(text) {
        Ok(hk) => (hk, None),
        Err(e) => (
            hotkey::parse(config::DEFAULT_HOTKEY).expect("既定のホットキーは正しい書き方"),
            Some(e),
        ),
    };
    (hk, hk.to_string(), warning)
}

/// トレイのツールチップ。使っているホットキーだけを出す。
fn tray_tip(active: Option<&hotkey::Active>) -> String {
    match active {
        Some(active) => format!("ClipEdit ({})", active.text),
        None => "ClipEdit (ホットキーなし)".to_string(),
    }
}

/// 2つ目の起動なら None。
fn single_instance() -> Option<HANDLE> {
    // SAFETY: 名前は w! で作った 0 終端の定数。GetLastError は CreateMutexW の直後に読む。
    // 2つ目の起動では、作ったハンドルを閉じてから戻る。
    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\ClipEdit-SingleInstance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(mutex);
            return None;
        }
        Some(mutex)
    }
}

fn open_editor() {
    if let Some(editor) = EDITOR.with(|e| e.get()) {
        editor.open(clipboard::read_text);
    }
}

fn toggle_autostart(hwnd: HWND) {
    if let Err(e) = autostart::set(!autostart::is_enabled()) {
        show_notice(
            hwnd,
            tray::NoticeKind::Warning,
            "ClipEdit: 自動起動を切り替えられませんでした",
            &e,
        );
    }
}

extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // ホットキーと、2回目に起動されたときの合図（台帳 D1）
        WM_HOTKEY | instance::WM_OPEN_REQUEST => {
            open_editor();
            LRESULT(0)
        }
        tray::WM_TRAY => {
            match (lparam.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP => open_editor(),
                WM_RBUTTONUP => tray::show_menu(hwnd, autostart::is_enabled()),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 & 0xFFFF {
                tray::CMD_OPEN => open_editor(),
                tray::CMD_AUTOSTART => toggle_autostart(hwnd),
                tray::CMD_RELOAD => reload_config(hwnd),
                // SAFETY: hwnd はこのウィンドウ関数が受け取った自分のウィンドウ。
                tray::CMD_EXIT => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(active) = ACTIVE_HOTKEY.with(|a| a.borrow_mut().take()) {
                hotkey::unregister(hwnd, active.id);
            }
            tray::remove(hwnd);
            // SAFETY: 値を渡すだけ。
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if msg == TASKBAR_CREATED.with(|t| t.get()) && msg != 0 => {
            // エクスプローラーが再起動したらトレイアイコンを付け直す
            add_tray_icon(hwnd);
            LRESULT(0)
        }
        // SAFETY: システムから受け取った引数を、そのまま既定の処理に渡す。
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hotkey_is_used_as_written() {
        let (hk, text, warning) = choose_hotkey("Ctrl+Shift+E");
        assert_eq!(hk, hotkey::parse("Ctrl+Shift+E").unwrap());
        assert_eq!(text, "Ctrl+Shift+E");
        assert_eq!(warning, None);
    }

    // 台帳 D42: ツールチップと通知には、設定の書き方ではなく決まった書き方を出す
    #[test]
    fn hotkey_text_is_canonical_not_as_written() {
        let (_, text, _) = choose_hotkey(" ctrl + shift + e ");
        assert_eq!(text, "Ctrl+Shift+E");
    }

    #[test]
    fn invalid_hotkey_falls_back_to_default_with_warning() {
        let (hk, text, warning) = choose_hotkey("Ctrl+Nope");
        assert_eq!(hk, hotkey::parse(config::DEFAULT_HOTKEY).unwrap());
        assert_eq!(text, config::DEFAULT_HOTKEY);
        assert!(warning.unwrap().to_lowercase().contains("nope"));
    }

    // 台帳 D9・D36: 設定を読み直したら、いま使えるホットキーを知らせる
    #[test]
    fn summary_reports_the_hotkey_in_use() {
        let registered = hotkey::Outcome::Registered("Ctrl+Shift+E".into());
        assert_eq!(
            hotkey_summary(&registered, None),
            "ホットキーは Ctrl+Shift+E です"
        );
        let kept = hotkey::Outcome::Kept("Ctrl+Shift+E".into());
        let warned = hotkey_summary(&kept, Some("書き方が正しくありません"));
        assert_eq!(
            warned,
            "書き方が正しくありません\nホットキーは Ctrl+Shift+E のままです"
        );
        let failed = hotkey::Outcome::Failed {
            wanted: "Ctrl+Shift+E".into(),
            kept: Some("Ctrl+Alt+V".into()),
        };
        assert_eq!(
            hotkey_summary(&failed, None),
            "ホットキー Ctrl+Shift+E を登録できませんでした。Ctrl+Alt+V のまま使えます。"
        );
        let none = hotkey::Outcome::Failed {
            wanted: "Ctrl+Alt+V".into(),
            kept: None,
        };
        assert_eq!(
            hotkey_summary(&none, None),
            "ホットキー Ctrl+Alt+V を登録できませんでした。トレイアイコンから開けます。"
        );
    }

    // 台帳 D34: うまくいった知らせだけを「情報」にし、警告の印を付けない
    #[test]
    fn reload_notice_is_info_only_when_all_went_well() {
        assert_eq!(reload_notice_kind(true, None), tray::NoticeKind::Info);
        assert_eq!(reload_notice_kind(false, None), tray::NoticeKind::Warning);
        assert_eq!(
            reload_notice_kind(true, Some("書き方が正しくありません")),
            tray::NoticeKind::Warning
        );
    }

    // 監査 S9: exe と同じフォルダに置かれた DLL を読まず、System32 の本物を読む
    #[test]
    fn dll_next_to_exe_is_not_loaded() {
        use windows::Win32::Foundation::FreeLibrary;
        use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, LoadLibraryW};
        // KnownDLLs に入っておらず、テストの exe がまだ読み込んでいない DLL
        let name = "XmlLite.dll";
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let system =
            std::path::PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32");
        let planted = exe_dir.join(name);
        std::fs::copy(system.join(name), &planted).unwrap();

        restrict_dll_search();
        // SAFETY: 名前は HSTRING（0 終端）。読み込んだモジュールは、パスを読んだあとで解放する。
        let loaded = unsafe {
            let module = LoadLibraryW(&HSTRING::from(name)).unwrap();
            let mut buf = [0u16; 260];
            let len = GetModuleFileNameW(Some(module), &mut buf) as usize;
            let _ = FreeLibrary(module);
            String::from_utf16_lossy(&buf[..len])
        };
        let _ = std::fs::remove_file(&planted);
        assert!(
            loaded
                .to_lowercase()
                .starts_with(&system.to_string_lossy().to_lowercase()),
            "exe のフォルダの写しが読まれた: {loaded}"
        );
    }

    // 台帳 D2: 起動できなかったときは、何に失敗したかと理由を画面に出す
    #[test]
    fn startup_error_names_what_failed_and_why() {
        let text = startup_error_text(
            "編集ウィンドウを作れませんでした",
            "アクセスが拒否されました",
        );
        assert!(text.contains("ClipEdit を起動できませんでした"));
        assert!(text.contains("編集ウィンドウを作れませんでした"));
        assert!(text.contains("アクセスが拒否されました"));
    }

    // 台帳 D18: 登録できなかったキーをツールチップに出し続けない
    #[test]
    fn tray_tip_shows_hotkey_only_when_registered() {
        let active = hotkey::Active {
            id: 1,
            key: hotkey::parse("Ctrl+Alt+V").unwrap(),
            text: "Ctrl+Alt+V".into(),
        };
        assert_eq!(tray_tip(Some(&active)), "ClipEdit (Ctrl+Alt+V)");
        assert_eq!(tray_tip(None), "ClipEdit (ホットキーなし)");
    }
}
