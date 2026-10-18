//! 2つ目に起動されたとき、すでに動いている `ClipEdit` に編集ウィンドウを開かせる（台帳 D1）。
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, PostMessageW, WM_APP,
};
use windows::core::PCWSTR;

/// 動いている `ClipEdit` の本体ウィンドウに送る「編集ウィンドウを開いて」の合図。
pub const WM_OPEN_REQUEST: u32 = WM_APP + 2;

/// 1つ目が単一起動の印を取ってから本体ウィンドウを作るまでの間に起動されたときのため、
/// 見つからなければ 100ms ごとに、合わせて約 2 秒まで探し直す（台帳 D37）。
const FIND_TRIES: u32 = 20;
const FIND_WAIT: Duration = Duration::from_millis(100);

/// 2回目の起動。forward で動いているほうに編集ウィンドウを開かせ、届かなければ tell で知らせる（台帳 D41）。
/// 届かないのは、1つ目が応答しないときや、単一起動の印をほかのアプリに取られたとき（台帳 D11）。
pub fn hand_over_to_running(forward: impl FnOnce() -> bool, tell: impl FnOnce(&str)) {
    if !forward() {
        tell(
            "ClipEdit はすでに動いているようですが、応答しません。\n\
             タスクマネージャーで clipedit.exe を終了してから、起動し直してください。",
        );
    }
}
/// class のウィンドウを探して開く合図を送る。見つからなければ false。
pub fn forward_to_running(class: PCWSTR) -> bool {
    forward_with_retries(class, FIND_TRIES, FIND_WAIT)
}

/// find を、見つかるまで最大 tries 回、wait ずつ間を空けて呼ぶ。
fn find_with_retries<T>(
    tries: u32,
    wait: Duration,
    mut find: impl FnMut() -> Option<T>,
) -> Option<T> {
    for attempt in 0..tries {
        if let Some(found) = find() {
            return Some(found);
        }
        if attempt + 1 < tries {
            std::thread::sleep(wait);
        }
    }
    None
}

fn forward_with_retries(class: PCWSTR, tries: u32, wait: Duration) -> bool {
    // SAFETY: class は 0 終端の文字列。見つかったウィンドウには値だけを送る。pid はこの関数のローカル変数。
    unsafe {
        let found = find_with_retries(tries, wait, || FindWindowW(class, PCWSTR::null()).ok());
        let Some(hwnd) = found else {
            return false;
        };
        // 起動したのは利用者の操作なので、前面に出す権利を動いているほうに譲る
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        PostMessageW(Some(hwnd), WM_OPEN_REQUEST, WPARAM(0), LPARAM(0)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
        PeekMessageW, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    };
    use windows::core::{HSTRING, PCWSTR};

    thread_local! {
        static RECEIVED: Cell<bool> = const { Cell::new(false) };
    }

    extern "system" fn test_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_OPEN_REQUEST {
            RECEIVED.with(|r| r.set(true));
            return LRESULT(0);
        }
        // SAFETY: システムから受け取った引数を、そのまま既定の処理に渡す。
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    #[test]
    fn second_launch_asks_running_instance_to_open() {
        let class = HSTRING::from(format!("ClipEditTestTarget{}", std::process::id()));
        // SAFETY: class は HSTRING（0 終端）で、テストが終わるまで生きている。test_proc は
        // extern "system" のウィンドウ関数。ウィンドウはこのスレッドで作り、最後に壊す。
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).unwrap().into();
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(test_proc),
                hInstance: instance,
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            });
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
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
            .unwrap()
        };
        assert!(forward_to_running(PCWSTR(class.as_ptr())));
        let mut msg = MSG::default();
        // SAFETY: msg はローカル変数。このスレッドのウィンドウへのメッセージだけを処理する。
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            let _ = DestroyWindow(hwnd);
        }
        assert!(RECEIVED.with(|r| r.get()), "開く合図が届いていない");
    }

    #[test]
    fn nothing_to_forward_when_no_instance_is_running() {
        assert!(!forward_with_retries(
            windows::core::w!("ClipEditNoSuchWindowClass"),
            2,
            Duration::ZERO
        ));
    }

    // 台帳 D41: 2回目の起動で動いているほうに届かなければ、黙って終わらずに知らせる
    #[test]
    fn second_launch_tells_only_when_the_running_one_is_not_reached() {
        let mut told = Vec::new();
        hand_over_to_running(|| true, |t| told.push(t.to_string()));
        assert!(told.is_empty(), "届いたときは何も出さない");
        hand_over_to_running(|| false, |t| told.push(t.to_string()));
        assert_eq!(told.len(), 1);
        assert!(told[0].contains("応答しません"), "{}", told[0]);
    }
    // 台帳 D37: 1つ目が本体ウィンドウを作り終える前でも、少し待って探し直す
    #[test]
    fn find_retries_until_found() {
        let mut calls = 0;
        let found = find_with_retries(5, Duration::ZERO, || {
            calls += 1;
            (calls == 3).then_some("window")
        });
        assert_eq!(found, Some("window"));
        assert_eq!(calls, 3, "見つかったらそれ以上探さない");
    }

    #[test]
    fn find_gives_up_after_the_last_try() {
        let mut calls = 0;
        let found: Option<()> = find_with_retries(4, Duration::ZERO, || {
            calls += 1;
            None
        });
        assert_eq!(found, None);
        assert_eq!(calls, 4);
    }
}
