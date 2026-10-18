//! Win32 に渡す 0 終端の UTF-16 と、Rust の文字列の変換（台帳 D20）。

/// 文字列を、0 終端の UTF-16 にする。
pub fn to_wide_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// UTF-16 の配列を、最初の 0 か配列の終わりまで文字列にする。
pub fn from_wide_nul(units: &[u16]) -> String {
    let len = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..len])
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
}
