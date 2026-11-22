use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("clipedit-favorites-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(listing: &Listing) -> Vec<&str> {
    listing.items.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn file_name_gives_number_and_name() {
    assert_eq!(
        parse_file_name("01 コードレビュー依頼.txt"),
        Some((Some(1), "コードレビュー依頼".into()))
    );
    assert_eq!(
        parse_file_name("7 要約.TXT"),
        Some((Some(7), "要約".into()))
    );
    assert_eq!(
        parse_file_name("100  英訳.txt"),
        Some((Some(100), "英訳".into())),
        "区切りの空白が2つでも名前に入れない"
    );
    assert_eq!(parse_file_name("メモ.txt"), Some((None, "メモ".into())));
    assert_eq!(
        parse_file_name("01要約.txt"),
        Some((None, "01要約".into())),
        "数字のあとに空白がなければ番号とみなさない"
    );
    assert_eq!(
        parse_file_name("05.txt"),
        Some((Some(5), "05".into())),
        "名前がなければ番号の文字をそのまま名前にする"
    );
    assert_eq!(
        parse_file_name("０１ 全角.txt"),
        Some((None, "０１ 全角".into())),
        "全角数字は番号にしない"
    );
    assert_eq!(parse_file_name("01 a.md"), None);
    assert_eq!(parse_file_name(".txt"), None);
}

#[test]
fn huge_number_is_treated_as_unnumbered() {
    assert_eq!(
        parse_file_name("99999999999999999999 a.txt"),
        Some((None, "99999999999999999999 a".into()))
    );
}

#[test]
fn missing_folder_gives_empty_listing() {
    assert_eq!(list(&temp_dir("missing").join("none")), Listing::default());
}

#[test]
fn listing_is_sorted_by_number_then_name_and_unnumbered_last() {
    let dir = temp_dir("sorted");
    for name in ["05 b.txt", "01 z.txt", "メモ.txt", "01 a.txt", "10 c.txt"] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    assert_eq!(names(&list(&dir)), ["a", "z", "b", "c", "メモ"]);
}

#[test]
fn text_is_read_as_utf8_with_bom_and_crlf() {
    let dir = temp_dir("bom");
    std::fs::write(dir.join("01 a.txt"), b"\xEF\xBB\xBFline1\nline2").unwrap();
    let listing = list(&dir);
    assert_eq!(listing.items[0].text, "line1\r\nline2");
}

#[test]
fn subfolders_and_other_extensions_are_ignored() {
    let dir = temp_dir("ignored");
    std::fs::write(dir.join("01 a.txt"), "x").unwrap();
    std::fs::create_dir(dir.join("02 b.txt")).unwrap();
    std::fs::write(dir.join("03 c.md"), "x").unwrap();
    let listing = list(&dir);
    assert_eq!(names(&listing), ["a"]);
    assert_eq!(listing.broken, 0);
}

#[test]
fn oversized_and_non_utf8_files_are_counted_as_broken() {
    let dir = temp_dir("broken");
    std::fs::write(dir.join("01 big.txt"), vec![b'x'; MAX_SIZE as usize + 1]).unwrap();
    std::fs::write(dir.join("02 bad.txt"), [0xFF, 0xFE, 0xFD]).unwrap();
    std::fs::write(dir.join("03 ok.txt"), "ok").unwrap();
    let listing = list(&dir);
    assert_eq!(names(&listing), ["ok"]);
    assert_eq!(listing.broken, 2);
}

#[test]
fn file_at_size_limit_is_read() {
    let dir = temp_dir("limit");
    std::fs::write(dir.join("01 a.txt"), vec![b'x'; MAX_SIZE as usize]).unwrap();
    let listing = list(&dir);
    assert_eq!(listing.items.len(), 1);
    assert_eq!(listing.broken, 0);
}

#[test]
fn utf16_file_is_counted_as_broken() {
    let dir = temp_dir("utf16");
    // メモ帳の「UTF-16 LE」: BOM FF FE + "あ"
    std::fs::write(dir.join("01 a.txt"), [0xFF, 0xFE, 0x42, 0x30]).unwrap();
    let listing = list(&dir);
    assert_eq!(listing.items, []);
    assert_eq!(listing.broken, 1);
}

#[test]
fn only_first_twenty_are_listed() {
    let dir = temp_dir("limit20");
    for i in 1..=23 {
        std::fs::write(dir.join(format!("{i:02} n{i:02}.txt")), "x").unwrap();
    }
    let listing = list(&dir);
    assert_eq!(listing.items.len(), 20);
    assert_eq!(listing.items[19].name, "n20");
    assert_eq!(listing.over_limit, 3);
}

// 20 本そろったあとのファイルは読まずに数えるだけ（監査 S20）。読めば「読み込めない」に入る壊れたファイルも、上限を超えた側に入る
#[test]
fn files_after_twenty_are_counted_without_reading() {
    let dir = temp_dir("limit20-unread");
    for i in 1..=20 {
        std::fs::write(dir.join(format!("{i:02} n{i:02}.txt")), "x").unwrap();
    }
    std::fs::write(dir.join("21 bad.txt"), [0xFF]).unwrap();
    let listing = list(&dir);
    assert_eq!(listing.items.len(), 20);
    assert_eq!(listing.over_limit, 1);
    assert_eq!(listing.broken, 0);
}

// 読めないファイルが多いときは、MAX_READS 個読んだところで読むのをやめる（監査 S20）
#[test]
fn stops_reading_after_max_reads() {
    let dir = temp_dir("max-reads");
    for i in 1..=MAX_READS + 1 {
        std::fs::write(dir.join(format!("{i:03} bad.txt")), [0xFF]).unwrap();
    }
    std::fs::write(dir.join("999 ok.txt"), "x").unwrap();
    let listing = list(&dir);
    assert!(listing.items.is_empty(), "上限より後ろは読まない");
    assert_eq!(listing.broken, MAX_READS + 2);
}

#[test]
fn unreadable_file_does_not_use_up_a_number() {
    let dir = temp_dir("gap");
    std::fs::write(dir.join("01 bad.txt"), [0xFF]).unwrap();
    std::fs::write(dir.join("02 ok.txt"), "x").unwrap();
    let listing = list(&dir);
    assert_eq!(names(&listing), ["ok"]);
    assert_eq!(listing.broken, 1);
}

#[test]
fn name_comes_from_first_non_empty_line() {
    assert_eq!(
        name_from_text("\r\n  \r\n  コードレビューを依頼します  \r\n本文"),
        "コードレビューを依頼します"
    );
    assert_eq!(
        name_from_text(r#"a\b/c:d*e?f"g<h>i|j"#),
        "a_b_c_d_e_f_g_h_i_j"
    );
    assert_eq!(
        name_from_text("tab\there"),
        "tab_here",
        "制御文字も置き換える"
    );
    assert_eq!(name_from_text(&"あ".repeat(40)), "あ".repeat(30));
    assert_eq!(name_from_text("終わりにピリオド. . "), "終わりにピリオド");
    assert_eq!(name_from_text(" \r\n\t"), "お気に入り");
    assert_eq!(name_from_text("..."), "お気に入り");
}

#[test]
fn next_number_follows_the_largest() {
    assert_eq!(next_number(&[]), 1);
    assert_eq!(next_number(&[1, 5, 3]), 6);
    assert_eq!(next_number(&[99]), 100);
}

#[test]
fn same_name_gets_a_suffix() {
    let taken = vec!["要約".to_string(), "要約 (2)".to_string()];
    assert_eq!(unique_name("要約", &taken), "要約 (3)");
    assert_eq!(unique_name("英訳", &taken), "英訳");
}

#[test]
fn add_saves_next_number_and_returns_position() {
    let dir = temp_dir("add-basic");
    for name in ["01 a.txt", "05 b.txt", "メモ.txt"] {
        std::fs::write(dir.join(name), "x").unwrap();
    }
    let added = add(&dir, "新しい\r\n本文");
    assert_eq!(added, Ok(3));
    assert_eq!(
        std::fs::read(dir.join("06 新しい.txt")).unwrap(),
        "新しい\r\n本文".as_bytes()
    );
    assert_eq!(list(&dir).items[2].name, "新しい");
}

#[test]
fn add_creates_the_folder_and_pads_to_two_digits() {
    let dir = temp_dir("add-new").join("favorites");
    assert_eq!(add(&dir, "x"), Ok(1));
    assert!(dir.join("01 x.txt").is_file());
}

#[test]
fn add_never_overwrites_and_suffixes_same_name() {
    let dir = temp_dir("add-same");
    std::fs::write(dir.join("01 要約.txt"), "元").unwrap();
    assert_eq!(add(&dir, "要約"), Ok(2));
    assert!(dir.join("02 要約 (2).txt").is_file());
    assert_eq!(
        std::fs::read_to_string(dir.join("01 要約.txt")).unwrap(),
        "元"
    );
}

#[test]
fn add_refuses_empty_full_and_oversized() {
    let dir = temp_dir("add-refuse");
    assert_eq!(
        add(&dir, " \r\n "),
        Err("空の内容はお気に入りに追加できません".to_string())
    );

    let full = "お気に入りは 20 本までです。「お気に入りを整理…」で減らしてください";
    for i in 1..=20 {
        std::fs::write(dir.join(format!("{i:02} n{i:02}.txt")), "x").unwrap();
    }
    assert_eq!(add(&dir, "new"), Err(full.to_string()));
    std::fs::write(dir.join("21 n21.txt"), "x").unwrap();
    assert_eq!(add(&dir, "new"), Err(full.to_string()));

    let dir = temp_dir("add-broken");
    for i in 1..=19 {
        std::fs::write(dir.join(format!("{i:02} n{i:02}.txt")), "x").unwrap();
    }
    for i in 90..93 {
        std::fs::write(dir.join(format!("{i} bad.txt")), [0xFF]).unwrap();
    }
    assert!(add(&dir, "new").is_ok(), "読めないものは数えない");

    let big = "a".repeat(MAX_SIZE as usize + 1);
    assert_eq!(
        add(&temp_dir("add-big"), &big),
        Err("お気に入りにできるのは 64 KB までです".to_string())
    );
}

#[test]
fn add_reports_error_when_folder_cannot_be_created() {
    let base = temp_dir("add-blocked");
    let dir = base.join("favorites");
    std::fs::write(&dir, "file").unwrap();
    let err = add(&dir, "x").unwrap_err();
    assert!(
        err.starts_with("お気に入りを保存できませんでした: "),
        "{err}"
    );
}

// 監査 S36: 末尾に \ がないと、ShellExecuteW は拡張子を補って隣の favorites.bat などを探して実行する
#[test]
fn folder_to_open_ends_with_separator() {
    use std::ffi::OsString;
    assert_eq!(
        folder_to_open(Path::new(r"C:\app\favorites")),
        OsString::from(r"C:\app\favorites\")
    );
    assert_eq!(
        folder_to_open(Path::new(r"C:\app\favorites\")),
        OsString::from(r"C:\app\favorites\"),
        "すでに付いていれば重ねない"
    );
    assert_eq!(
        folder_to_open(Path::new(r"\server\share\お気に入り")),
        OsString::from(r"\server\share\お気に入り\")
    );
}

// 監査 S38: 同じ番号と名前になるファイル同士は、ファイル名の順に並べる（フォルダを読む順に頼らない）
#[test]
fn same_number_and_name_are_ordered_by_file_name() {
    let dir = temp_dir("tie");
    std::fs::write(dir.join("1 a.txt"), "second").unwrap();
    std::fs::write(dir.join("01 a.txt"), "first").unwrap();
    let listing = list(&dir);
    let texts: Vec<&str> = listing.items.iter().map(|f| f.text.as_str()).collect();
    assert_eq!(texts, ["first", "second"]);
}
