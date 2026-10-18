use std::time::Duration;

use crate::wide;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::core::{PCWSTR, w};

const RETRIES: u32 = 5;
const RETRY_WAIT: Duration = Duration::from_millis(50);

// パスワード管理ソフトなどが「履歴・クラウド・監視ソフトに残さない」ために置く形式
const EXCLUDE_FORMAT: PCWSTR = w!("ExcludeClipboardContentFromMonitorProcessing");
const HISTORY_FORMAT: PCWSTR = w!("CanIncludeInClipboardHistory");
const CLOUD_FORMAT: PCWSTR = w!("CanUploadToCloudClipboard");

/// クリップボードから読んだテキスト。private はパスワードのように履歴へ残さない印が付いていたか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipText {
    pub text: String,
    pub private: bool,
}

/// 3つの印のどれか1つでも「残さない」なら private。
fn is_private(exclude: bool, history: Option<u32>, cloud: Option<u32>) -> bool {
    exclude || history == Some(0) || cloud == Some(0)
}

/// 開いている間だけ存在し、drop で `CloseClipboard` する。
struct OpenGuard;

impl OpenGuard {
    fn open(owner: Option<HWND>) -> Result<OpenGuard, String> {
        for attempt in 0..RETRIES {
            // SAFETY: owner は None か、このスレッドが作った有効なウィンドウ。
            // 開けたときは OpenGuard が drop で必ず閉じる。
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(OpenGuard);
            }
            if attempt + 1 < RETRIES {
                std::thread::sleep(RETRY_WAIT);
            }
        }
        Err("クリップボードを開けませんでした".to_string())
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        // SAFETY: OpenGuard は OpenClipboard が成功したときだけ作られるので、ここで閉じるのは正しい。
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// クリップボードのテキストを読む。テキストが無ければ Ok(None)。
pub fn read_text() -> Result<Option<ClipText>, String> {
    let _guard = OpenGuard::open(None)?;
    // SAFETY: _guard がある間はクリップボードが開いており、GetClipboardData のハンドルは有効。
    // ロックしたメモリは GlobalSize が返す大きさ（確保された大きさ）までしか読まず、
    // 読み終えてから GlobalUnlock する。ハンドル自体はシステムのものなので解放しない。
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
            return Ok(None);
        }
        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).map_err(|e| e.message())?;
        let global = HGLOBAL(handle.0);
        let ptr = GlobalLock(global) as *const u16;
        if ptr.is_null() {
            return Err("クリップボードの内容を読めませんでした".to_string());
        }
        // 終端の 0 が無いデータでも、確保されたサイズより先は読まない
        let capacity = GlobalSize(global) / 2;
        let text = wide::from_wide_nul(std::slice::from_raw_parts(ptr, capacity));
        let _ = GlobalUnlock(global);
        let private = is_private(
            IsClipboardFormatAvailable(RegisterClipboardFormatW(EXCLUDE_FORMAT)).is_ok(),
            read_dword(HISTORY_FORMAT),
            read_dword(CLOUD_FORMAT),
        );
        Ok(Some(ClipText { text, private }))
    }
}

/// 開いているクリップボードから、name の形式の DWORD を読む。無ければ None。
fn read_dword(name: PCWSTR) -> Option<u32> {
    // SAFETY: name は w! で作った 0 終端の文字列。クリップボードが開いていなければ GetClipboardData は
    // 失敗するだけ。4 バイト以上あることを確かめてから、境界をそろえずに 4 バイトだけ読む。
    unsafe {
        let handle = GetClipboardData(RegisterClipboardFormatW(name)).ok()?;
        let global = HGLOBAL(handle.0);
        if GlobalSize(global) < 4 {
            return None;
        }
        let ptr = GlobalLock(global) as *const u32;
        if ptr.is_null() {
            return None;
        }
        let value = ptr.read_unaligned();
        let _ = GlobalUnlock(global);
        Some(value)
    }
}

/// bytes を複製した移動可能なメモリを確保する。
fn alloc_global(bytes: &[u8]) -> Result<HGLOBAL, String> {
    // SAFETY: 確保した bytes.len() バイトのメモリに bytes.len() バイトだけ写す。
    // ロックに失敗したときは、確保したメモリを解放してから戻る。
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.message())?;
        let ptr = GlobalLock(global) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return Err("メモリを確保できませんでした".to_string());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(global);
        Ok(global)
    }
}

/// クリップボードをテキストで置き換える。owner はクリップボードの所有者にするウィンドウ。
/// private なら、履歴・クラウド・監視ソフトに残さない印も一緒に置く。
pub fn write_text(owner: Option<HWND>, text: &str, private: bool) -> Result<(), String> {
    let text_bytes: Vec<u8> = wide::to_wide_nul(text)
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    // 印を本文より先に置く。途中で失敗しても「印のない本文」が残らないようにするため（台帳 D17）
    let mut items: Vec<(u32, &[u8])> = Vec::new();
    let zero = 0u32.to_le_bytes();
    if private {
        for name in [EXCLUDE_FORMAT, HISTORY_FORMAT, CLOUD_FORMAT] {
            // SAFETY: name は w! で作った 0 終端の文字列。
            items.push((unsafe { RegisterClipboardFormatW(name) }, &zero));
        }
    }
    items.push((CF_UNICODETEXT.0 as u32, &text_bytes));

    // メモリはクリップボードを空にする前に確保する。確保に失敗しても元の内容は消えない（台帳 D4）
    let mut globals = Vec::new();
    for (format, bytes) in &items {
        match alloc_global(bytes) {
            Ok(global) => globals.push((*format, global)),
            Err(e) => {
                free_all(&globals);
                return Err(e);
            }
        }
    }

    let _guard = match OpenGuard::open(owner) {
        Ok(guard) => guard,
        Err(e) => {
            free_all(&globals);
            return Err(e);
        }
    };
    // SAFETY: _guard がある間はクリップボードが開いている。SetClipboardData が成功したメモリは
    // システムのものになるので、失敗したときは、まだ渡していないメモリだけを解放する。
    unsafe {
        if let Err(e) = EmptyClipboard() {
            free_all(&globals);
            return Err(e.message());
        }
        for (i, (format, global)) in globals.iter().enumerate() {
            if let Err(e) = SetClipboardData(*format, Some(HANDLE(global.0))) {
                free_all(&globals[i..]);
                return Err(e.message());
            }
        }
    }
    Ok(())
}

/// まだクリップボードに渡していないメモリを解放する。
fn free_all(globals: &[(u32, HGLOBAL)]) {
    for (_, global) in globals {
        // SAFETY: どれも alloc_global で確保し、まだシステムに渡していないメモリ。
        unsafe {
            let _ = GlobalFree(Some(*global));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_when_any_mark_says_so() {
        assert!(!is_private(false, None, None), "印なし");
        assert!(is_private(true, None, None), "履歴除外の形式がある");
        assert!(is_private(false, Some(0), None), "履歴に入れない");
        assert!(is_private(false, None, Some(0)), "クラウドに上げない");
        assert!(!is_private(false, Some(1), Some(1)), "どちらも許可");
    }

    // 実際のクリップボードを書き換えるので通常は実行しない: cargo test -- --ignored --test-threads=1
    #[test]
    #[ignore = "実際のクリップボードを書き換える"]
    fn empty_clipboard_reads_as_no_text() {
        {
            let _guard = OpenGuard::open(None).unwrap();
            // SAFETY: クリップボードは _guard で開いている。
            unsafe { EmptyClipboard() }.unwrap();
        }
        assert_eq!(read_text(), Ok(None));
    }

    #[test]
    #[ignore = "実際のクリップボードを書き換える"]
    fn round_trips_text() {
        for sample in ["日本語のテキスト", "絵文字 😀👍", "1行目\r\n2行目\r\n", ""]
        {
            write_text(None, sample, false).unwrap();
            let read = read_text().unwrap().unwrap();
            assert_eq!(read.text, sample);
            assert!(!read.private);
        }
    }

    // 印を本文より先に置けば、途中で失敗しても「印のない本文」は残らない（台帳 D17）。
    // EnumClipboardFormats は置いた順に返す。
    #[test]
    #[ignore = "実際のクリップボードを書き換える"]
    fn private_text_places_marks_before_text() {
        use windows::Win32::System::DataExchange::EnumClipboardFormats;
        write_text(None, "P@ssw0rd", true).unwrap();
        let _guard = OpenGuard::open(None).unwrap();
        let mut order = Vec::new();
        let mut format = 0;
        loop {
            // SAFETY: クリップボードは _guard で開いている。値を渡すだけ。
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            order.push(format);
        }
        let position = |f: u32| order.iter().position(|&x| x == f).unwrap();
        let text = position(CF_UNICODETEXT.0 as u32);
        for name in [EXCLUDE_FORMAT, HISTORY_FORMAT, CLOUD_FORMAT] {
            // SAFETY: name は w! で作った 0 終端の文字列。
            let mark = position(unsafe { RegisterClipboardFormatW(name) });
            assert!(mark < text, "印 {mark} が本文 {text} より後に置かれている");
        }
    }

    #[test]
    #[ignore = "実際のクリップボードを書き換える"]
    fn private_text_keeps_exclusion_marks() {
        write_text(None, "P@ssw0rd", true).unwrap();
        let read = read_text().unwrap().unwrap();
        assert_eq!(read.text, "P@ssw0rd");
        assert!(read.private);
        for name in [
            w!("ExcludeClipboardContentFromMonitorProcessing"),
            w!("CanIncludeInClipboardHistory"),
            w!("CanUploadToCloudClipboard"),
        ] {
            // SAFETY: name は w! で作った 0 終端の文字列。
            let format = unsafe { RegisterClipboardFormatW(name) };
            // SAFETY: 引数は値だけで、ポインタを渡さない。
            assert!(unsafe { IsClipboardFormatAvailable(format) }.is_ok());
        }
    }
}
