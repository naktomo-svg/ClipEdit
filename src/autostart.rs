use std::path::Path;

use crate::wide;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("ClipEdit");

/// Run キーに登録するコマンド行。パスに空白があっても動くよう引用符で囲む。
pub fn command_line(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Run キーの値 value が exe を起動するものか。
fn points_to(value: &str, exe: &Path) -> bool {
    value.to_lowercase() == command_line(exe).to_lowercase()
}

/// Run キーに、今動いている exe が登録されているか。
/// exe を移動した後の古い登録は「無効」と扱い、メニューから選び直せば今のパスで登録し直す。
pub fn is_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    read_value().is_some_and(|value| points_to(&value, &exe))
}

fn read_value() -> Option<String> {
    let mut size = 0u32;
    // SAFETY: 名前はどれも w! で作った 0 終端の文字列。1回目はデータを受け取らず大きさだけを size に受け取る。
    // 2回目は size バイト以上ある buf を渡し、RegGetValueW は size を超えて書かない。
    // 間に値が大きくなっていれば ERROR_MORE_DATA で失敗し、None を返す。
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        Some(wide::from_wide_nul(&buf))
    }
}

/// Run キーに value を書く。
fn write_value(value: &str) -> Result<(), String> {
    let wide = wide::to_wide_nul(value);
    // SAFETY: wide は 0 終端までを含む UTF-16 の配列で、渡す大きさ（バイト数）はその長さと一致する。
    let result = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
    };
    result.ok().map_err(|e| e.message())
}

pub fn set(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        write_value(&command_line(&exe))
    } else {
        // SAFETY: 名前はどれも w! で作った 0 終端の文字列。
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        result.ok().map_err(|e| e.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 作ったときの Run キーの値を覚え、drop で元に戻す（テストがパニックしても戻る）。
    struct RestoreRunValue(Option<String>);

    impl RestoreRunValue {
        fn save() -> RestoreRunValue {
            RestoreRunValue(read_value())
        }
    }

    impl Drop for RestoreRunValue {
        fn drop(&mut self) {
            let _ = match &self.0 {
                Some(value) => write_value(value),
                None => set(false),
            };
        }
    }

    #[test]
    fn command_line_is_quoted() {
        let exe = Path::new(r"C:\Program Files\ClipEdit\clipedit.exe");
        assert_eq!(
            command_line(exe),
            r#""C:\Program Files\ClipEdit\clipedit.exe""#
        );
    }

    #[test]
    fn registered_only_when_value_points_to_this_exe() {
        let exe = Path::new(r"C:\Tools\ClipEdit\clipedit.exe");
        assert!(points_to(r#""C:\Tools\ClipEdit\clipedit.exe""#, exe));
        assert!(
            points_to(r#""c:\tools\clipedit\CLIPEDIT.EXE""#, exe),
            "大文字小文字は区別しない"
        );
        assert!(
            !points_to(r#""C:\Users\me\Downloads\clipedit.exe""#, exe),
            "移動前の古いパス"
        );
        assert!(!points_to("", exe));
    }

    // 実際のレジストリを書き換えるので通常は実行しない（終わったら元の値に戻す）
    #[test]
    #[ignore = "実際の自動起動の設定（レジストリ）を書き換える"]
    fn stale_registration_is_not_enabled() {
        let _restore = RestoreRunValue::save();
        write_value("\"C:\\old\\clipedit.exe\"").unwrap();
        assert!(!is_enabled(), "古いパスの登録");
        set(true).unwrap();
        assert!(is_enabled(), "選び直すと今のパスで登録される");
    }

    // 登録していないときにオフにしても、エラーにしない
    #[test]
    #[ignore = "実際の自動起動の設定（レジストリ）を書き換える"]
    fn disabling_twice_is_ok() {
        let _restore = RestoreRunValue::save();
        assert_eq!(set(false), Ok(()));
        assert_eq!(set(false), Ok(()), "登録が無い状態でオフにする");
        assert!(!is_enabled());
    }

    // テストが途中で失敗しても、Run キーは元の値に戻る（台帳 D24）
    #[test]
    #[ignore = "実際の自動起動の設定（レジストリ）を書き換える"]
    fn run_value_is_restored_after_a_failed_test() {
        let before = read_value();
        let result = std::panic::catch_unwind(|| {
            let _restore = RestoreRunValue::save();
            write_value("\"C:\\old\\clipedit.exe\"").unwrap();
            panic!("テストの途中で失敗した");
        });
        assert!(result.is_err());
        assert_eq!(read_value(), before);
    }
}
