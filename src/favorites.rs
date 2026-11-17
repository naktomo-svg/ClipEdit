use crate::config::read_limited;
use crate::textops::normalize_newlines;
use std::io::Write;
use std::path::{Path, PathBuf};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

/// 呼び出せるお気に入りの数。これを超えたぶんは読まない。
pub const MAX_ITEMS: usize = 20;
/// これより大きいお気に入りは読まない（巨大なファイルでメモリを使い切らないように）。
pub const MAX_SIZE: u64 = 64 * 1024;
/// 一覧を作るときに中身を読むファイルの数の上限。読めないファイルが大量にあっても、
/// メニューを出すのに時間がかからないようにする（監査 S20）。
pub const MAX_READS: usize = 100;

/// フォルダの中の1本のお気に入り。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Favorite {
    pub name: String,
    /// 改行は CRLF にそろえてある。
    pub text: String,
}

/// フォルダを読んだ結果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// 並べた順。位置がそのまま呼び出しの番号になる。
    pub items: Vec<Favorite>,
    /// 大きすぎる・UTF-8 でないなど、読めなかったファイルの数。`MAX_READS` を超えて読まなかったものも含む。
    pub broken: usize,
    /// `MAX_ITEMS` 本そろったあとにあったので、読まずに入れなかったファイルの数。
    pub over_limit: usize,
}

/// フォルダにある1本の .txt。並べてから中身を読む。
struct Entry {
    number: Option<u32>,
    name: String,
    path: PathBuf,
    /// フォルダを読んだときに分かった大きさ。分からなければ None。
    size: Option<u64>,
}

/// exe と同じフォルダの favorites。
pub fn default_dir() -> PathBuf {
    crate::config::next_to_exe("favorites")
}

/// ファイル名を（並べる番号, 表示する名前）にする。`.txt` 以外は None。
/// 「01 名前.txt」の 01 が番号。数字のあとに半角空白がなければ番号ではなく名前の一部。
pub fn parse_file_name(file_name: &str) -> Option<(Option<u32>, String)> {
    let stem_len = file_name.len().checked_sub(4)?;
    if !file_name.get(stem_len..)?.eq_ignore_ascii_case(".txt") {
        return None;
    }
    let stem = &file_name[..stem_len];
    if stem.is_empty() {
        return None;
    }
    let digits_len = stem.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &stem[digits_len..];
    // 数字だけの名前（05.txt）も、数字のあとに空白がある名前も番号つき
    if digits_len > 0
        && (rest.is_empty() || rest.starts_with(' '))
        && let Ok(number) = stem[..digits_len].parse::<u32>()
    {
        let name = rest.trim_start_matches(' ');
        let name = if name.is_empty() {
            &stem[..digits_len]
        } else {
            name
        };
        return Some((Some(number), name.to_string()));
    }
    Some((None, stem.to_string()))
}

/// フォルダのお気に入りを、番号順（番号なしは後ろ）、同じ番号は名前順に読む。
/// フォルダが無い・開けないときは空の結果を返す。
pub fn list(dir: &Path) -> Listing {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Listing::default();
    };
    // 大きさはフォルダを読んだときの情報で分かる（ファイルを開かない）
    let mut files: Vec<Entry> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| {
            let (number, name) = parse_file_name(entry.file_name().to_str()?)?;
            let size = entry.metadata().ok().map(|m| m.len());
            Some(Entry {
                number,
                name,
                path: entry.path(),
                size,
            })
        })
        .collect();
    files.sort_by(|a, b| {
        // 同じ番号と名前（01 a.txt と 1 a.txt）は、ファイル名の順にする（フォルダを読む順に頼らない。監査 S38）
        (a.number.is_none(), a.number, &a.name, &a.path).cmp(&(
            b.number.is_none(),
            b.number,
            &b.name,
            &b.path,
        ))
    });

    let mut listing = Listing::default();
    let mut reads = 0;
    for Entry {
        name, path, size, ..
    } in files
    {
        // 20 本そろったら、残りは読まずに数えるだけにする（監査 S20）
        if listing.items.len() >= MAX_ITEMS {
            listing.over_limit += 1;
            continue;
        }
        // 大きすぎるものは開かない。読む数が上限に達したら、残りは読めなかったものとして数える
        if size.is_some_and(|size| size > MAX_SIZE) || reads >= MAX_READS {
            listing.broken += 1;
            continue;
        }
        reads += 1;
        // 読めないファイルは番号を消費しないよう、並べたあとに読む
        match read_limited(&path, MAX_SIZE) {
            Ok(Some(text)) => {
                let text = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
                listing.items.push(Favorite {
                    name,
                    text: normalize_newlines(text),
                });
            }
            Ok(None) | Err(_) => listing.broken += 1,
        }
    }
    listing
}

/// 追加するときの名前。最初の空でない行を、ファイル名に使える形にして30文字までにする。
pub fn name_from_text(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let name: String = line
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(30)
        .collect();
    // Windows はファイル名の末尾の空白とピリオドを削ってしまう
    let name = name.trim_end_matches([' ', '.']);
    if name.is_empty() {
        "お気に入り".to_string()
    } else {
        name.to_string()
    }
}

/// 次に付ける番号。今ある最大の番号の次（なければ 1）。
pub fn next_number(numbers: &[u32]) -> u32 {
    numbers.iter().max().map_or(1, |max| max.saturating_add(1))
}

/// 今ある名前と重ならない名前にする。重なるときは「名前 (2)」「名前 (3)」…。
pub fn unique_name(name: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == name) {
        return name.to_string();
    }
    (2..)
        .map(|n| format!("{name} ({n})"))
        .find(|candidate| !taken.contains(candidate))
        .expect("無限の列から必ず見つかる")
}

/// 今の内容を新しいお気に入りとして保存する。
/// 成功なら、一覧での位置（1 から数える）と付けた名前を返す。
pub fn add(dir: &Path, text: &str) -> Result<(usize, String), String> {
    if text.trim().is_empty() {
        return Err("空の内容はお気に入りに追加できません".to_string());
    }
    if text.len() as u64 > MAX_SIZE {
        return Err(format!(
            "お気に入りにできるのは {} KB までです",
            MAX_SIZE / 1024
        ));
    }
    let listing = list(dir);
    if listing.items.len() + listing.over_limit >= MAX_ITEMS {
        return Err(format!(
            "お気に入りは {MAX_ITEMS} 本までです。「お気に入りを整理…」で減らしてください"
        ));
    }

    // 番号と名前は、読めないファイルも含めて、フォルダにある .txt すべてから決める
    let mut numbers = Vec::new();
    let mut taken = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let Some((number, name)) = entry.file_name().to_str().and_then(parse_file_name) else {
                continue;
            };
            numbers.extend(number);
            taken.push(name);
        }
    }
    let number = next_number(&numbers);
    let name = unique_name(&name_from_text(text), &taken);

    let save_error = |e: std::io::Error| format!("お気に入りを保存できませんでした: {e}");
    std::fs::create_dir_all(dir).map_err(save_error)?;
    let path = dir.join(format!("{number:02} {name}.txt"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(save_error)?;
    if let Err(e) = file.write_all(text.as_bytes()) {
        // 書きかけのファイルを残さない
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(save_error(e));
    }
    drop(file);

    // 保存はできているので、一覧で見つからなくても成功として返す（末尾に入った想定）
    let listing = list(dir);
    let position = listing
        .items
        .iter()
        .position(|f| f.name == name)
        .map_or(listing.items.len() + 1, |p| p + 1);
    Ok((position, name))
}

/// お気に入りのフォルダをエクスプローラで開く。無ければ作る。
/// `ShellExecuteW` に渡すフォルダのパス。末尾に `\` を付ける（監査 S36）。
/// 付けないと、そのフォルダがないときに `PATHEXT` の拡張子を補って探し、隣の `favorites.bat` などを実行する。
fn folder_to_open(dir: &Path) -> std::ffi::OsString {
    let mut path = dir.as_os_str().to_owned();
    if !path.to_string_lossy().ends_with(['\\', '/']) {
        path.push("\\");
    }
    path
}

pub fn open_folder(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("お気に入りのフォルダを作れませんでした: {e}"))?;
    let full = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let wide = crate::wide::os_to_wide_nul(&folder_to_open(&full));
    // SAFETY: wide は 0 終端の UTF-16 文字列で、呼び出しの間は生きている。w! は静的な文字列。
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // 32 以下は失敗（ShellExecute の決まり）
    if result.0 as isize <= 32 {
        return Err(format!(
            "お気に入りのフォルダを開けませんでした（{}）",
            full.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
