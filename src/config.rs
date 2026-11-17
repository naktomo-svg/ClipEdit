use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+V";
/// これより大きい config.toml は読まない（巨大なファイルでメモリを使い切らないように）。
const MAX_SIZE: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: DEFAULT_HOTKEY.to_string(),
        }
    }
}

/// TOML の文字列を解釈する。書式が不正なら Err にエラー内容を返す。
pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| describe_error(text, &e))
}

/// toml のエラーを「N 行目: 説明」の1行にする。通知に出すので、元の行と ^ の図は付けない（台帳 D35）。
fn describe_error(text: &str, error: &toml::de::Error) -> String {
    let message = error
        .message()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match error.span() {
        Some(span) => {
            let before = &text.as_bytes()[..span.start.min(text.len())];
            // 改行で区切った数 = 改行の数 + 1
            let line = before.split(|&b| b == b'\n').count();
            format!("{line} 行目: {message}")
        }
        None => message,
    }
}

/// 設定ファイルを読む。ファイルが無ければ既定値、壊れていれば既定値と警告文を返す。
pub fn load(path: &Path) -> (Config, Option<String>) {
    match read_limited(path, MAX_SIZE) {
        Ok(None) => (
            Config::default(),
            Some(format!(
                "config.toml が大きすぎます（{} KB まで）",
                MAX_SIZE / 1024
            )),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => (
            Config::default(),
            Some(
                "config.toml を読めませんでした。文字コードを UTF-8 にして保存してください"
                    .to_string(),
            ),
        ),
        Err(e) => (
            Config::default(),
            Some(format!("config.toml を読めませんでした: {e}")),
        ),
        Ok(Some(text)) => match parse(&text) {
            Ok(config) => (config, None),
            Err(e) => (
                Config::default(),
                Some(format!("config.toml を読めませんでした: {e}")),
            ),
        },
    }
}

/// `max` バイトまでのファイルを UTF-8 として読む。それより大きければ `Ok(None)`。
pub(crate) fn read_limited(path: &Path, max: u64) -> std::io::Result<Option<String>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// exe と同じフォルダの name。exe の場所が分からなければ name だけ。
pub(crate) fn next_to_exe(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// exe と同じフォルダの config.toml。
pub fn default_path() -> PathBuf {
    next_to_exe("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join("clipedit-config-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        if let Some(content) = content {
            std::fs::write(&path, content).unwrap();
        }
        path
    }

    #[test]
    fn next_to_exe_is_in_the_exe_folder() {
        assert!(next_to_exe("config.toml").ends_with("config.toml"));
        assert_eq!(
            next_to_exe("x").parent(),
            std::env::current_exe().unwrap().parent()
        );
    }

    #[test]
    fn missing_file_gives_default_without_warning() {
        let path = temp_file("missing.toml", None);
        assert_eq!(load(&path), (Config::default(), None));
    }

    #[test]
    fn valid_file_is_read() {
        let path = temp_file("valid.toml", Some("hotkey = \"Ctrl+Shift+E\"\n"));
        let (config, warning) = load(&path);
        assert_eq!(config.hotkey, "Ctrl+Shift+E");
        assert_eq!(warning, None);
    }

    #[test]
    fn empty_file_gives_default() {
        let path = temp_file("empty.toml", Some(""));
        assert_eq!(load(&path), (Config::default(), None));
    }

    #[test]
    fn broken_file_gives_default_with_warning() {
        let path = temp_file("broken.toml", Some("hotkey = \n"));
        let (config, warning) = load(&path);
        assert_eq!(config, Config::default());
        assert!(warning.unwrap().contains("config.toml"));
    }

    // 台帳 D35: 通知に出すので、toml のエラー文の図（元の行と ^）は出さず、何行目かと短い説明だけにする
    #[test]
    fn syntax_error_warning_is_one_line_with_line_number() {
        let path = temp_file("syntax.toml", Some("# コメント\nhotkey = \n"));
        let warning = load(&path).1.unwrap();
        assert!(warning.contains("2 行目"), "{warning}");
        assert!(!warning.contains('\n'), "{warning}");
        assert!(!warning.contains('^'), "{warning}");
    }

    #[test]
    fn wrong_type_gives_default_with_warning() {
        let path = temp_file("wrongtype.toml", Some("hotkey = 5\n"));
        let (config, warning) = load(&path);
        assert_eq!(config, Config::default());
        assert!(warning.is_some());
    }

    #[test]
    fn utf8_bom_is_accepted() {
        let path = temp_file("bom.toml", Some("\u{FEFF}hotkey = \"Ctrl+Shift+E\"\n"));
        assert_eq!(load(&path).0.hotkey, "Ctrl+Shift+E");
    }

    #[test]
    fn oversized_file_gives_default_with_warning() {
        let big = format!(
            "hotkey = \"Ctrl+Shift+E\"\n#{}\n",
            "x".repeat(MAX_SIZE as usize)
        );
        let path = temp_file("big.toml", Some(&big));
        let (config, warning) = load(&path);
        assert_eq!(config, Config::default());
        assert!(warning.unwrap().contains("大きすぎます"));
    }

    #[test]
    fn file_at_size_limit_is_read() {
        let head = "hotkey = \"Ctrl+Shift+E\"\n#";
        let text = format!("{head}{}", "x".repeat(MAX_SIZE as usize - head.len()));
        let path = temp_file("limit.toml", Some(&text));
        assert_eq!(
            load(&path),
            (
                Config {
                    hotkey: "Ctrl+Shift+E".into()
                },
                None
            )
        );
    }

    #[test]
    fn non_utf8_file_gives_default_with_warning() {
        let path = temp_file("sjis.toml", None);
        // Shift_JIS で「# 日本」と書かれたファイル
        std::fs::write(&path, b"hotkey = \"Ctrl+Shift+E\" # \x93\xfa\x96\x7b\n").unwrap();
        let (config, warning) = load(&path);
        assert_eq!(config, Config::default());
        assert!(warning.unwrap().contains("UTF-8"));
    }
}
