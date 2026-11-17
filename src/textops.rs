//! 文字列を受け取って返すだけの処理（ウィンドウを使わない）。

/// 改行を CRLF にそろえる（LF だけ・CR だけの改行も EDIT コントロールで改行として見えるように）。
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// 折り返して表示すると開くのが遅くなる1行の長さ（文字数）。これより長い行があるテキストは、
/// 折り返さない編集欄で開く（監査 S10・台帳 D46）。折り返すと、空白も改行もない行は長さの2乗に近く遅くなる
/// （開くまで 1 万文字 0.5 秒、2 万文字 1.4 秒、5 万文字 6.5 秒）。この上限ちょうどでは 0.3 秒。
pub const WRAP_LIMIT: usize = 5_000;

/// text に、`WRAP_LIMIT` 文字より長い行があるか。
pub fn has_long_line(text: &str) -> bool {
    // バイト数は文字数以上なので、バイト数で足りる行は数えずに済ませる
    text.split(['\r', '\n'])
        .any(|line| line.len() > WRAP_LIMIT && line.chars().count() > WRAP_LIMIT)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
