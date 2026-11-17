//! Win32 に渡す 0 終端の UTF-16 と、Rust の文字列の変換（台帳 D20）。

use std::os::windows::ffi::OsStrExt;

/// 文字列を、0 終端の UTF-16 にする。
pub fn to_wide_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// UTF-16 の配列を、最初の 0 か配列の終わりまで文字列にする。
pub fn from_wide_nul(units: &[u16]) -> String {
    let len = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..len])
}

/// OS の文字列（パスなど）を、0 終端の UTF-16 にする。UTF-8 でないパスも失わない。
pub fn os_to_wide_nul(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

/// 固定長の UTF-16 配列に文字列を詰める（はみ出す分は文字の単位で切り捨て、末尾は必ず 0）。
pub fn copy_wide(dst: &mut [u16], src: &str) {
    // 書くところがなければ何もしない（0 終端も置けない。監査 S38）
    if dst.is_empty() {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_terminating_nul() {
        assert_eq!(to_wide_nul("aあ"), vec![b'a' as u16, 0x3042, 0]);
        assert_eq!(to_wide_nul(""), vec![0]);
    }

    #[test]
    fn stops_at_first_nul() {
        let units: Vec<u16> = "ab\0cd".encode_utf16().collect();
        assert_eq!(from_wide_nul(&units), "ab");
    }

    #[test]
    fn unterminated_buffer_is_read_only_to_its_end() {
        let units: Vec<u16> = "終端なし".encode_utf16().collect();
        assert_eq!(from_wide_nul(&units), "終端なし");
    }

    #[test]
    fn round_trips() {
        for s in ["", "日本語", "絵文字 😀"] {
            assert_eq!(from_wide_nul(&to_wide_nul(s)), s);
        }
    }

    #[test]
    fn os_string_gets_terminating_nul() {
        let os = std::ffi::OsStr::new("C:\\お気に入り");
        assert_eq!(os_to_wide_nul(os), to_wide_nul("C:\\お気に入り"));
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

    // 監査 S38: 長さ 0 の入れ物でもパニックしない（書くところがないので何もしない）
    #[test]
    fn copy_wide_into_empty_buffer_does_nothing() {
        let mut buf: [u16; 0] = [];
        copy_wide(&mut buf, "abc");
    }
}
