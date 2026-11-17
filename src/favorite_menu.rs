use std::path::Path;

use crate::favorites::{self, Listing, MAX_ITEMS};
use crate::tray;
use crate::wide::to_wide_nul;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DeleteMenu, DestroyMenu, GetMenuItemCount, HMENU, MF_BYPOSITION,
    MF_GRAYED, MF_SEPARATOR, MF_STRING,
};
use windows::core::PCWSTR;

/// お気に入りのコマンド ID の先頭（100〜119 が 1〜20 番目）。トレイの `CMD_*`（1〜4）と重ならない。
pub const CMD_FAVORITE_FIRST: usize = 100;
pub const CMD_ADD: usize = CMD_FAVORITE_FIRST + MAX_ITEMS;
pub const CMD_ORGANIZE: usize = CMD_ADD + 1;

// トレイのコマンド ID のうち最大の `CMD_RELOAD` より大きい（衝突はコンパイル時に防ぐ。トレイにコマンドを足したら、この比較も直す）
const _: () = assert!(CMD_FAVORITE_FIRST > crate::tray::CMD_RELOAD);

/// お気に入りのフォルダを開けなかったときの通知のタイトル。
const OPEN_FOLDER_FAILED_TITLE: &str = "ClipEdit: お気に入りのフォルダを開けませんでした";

/// 「お気に入りを整理…」。フォルダを開き、開けなければ（hwnd があれば）そのトレイから警告を出す。
pub fn organize(dir: &Path, hwnd: Option<HWND>) {
    if let Err(e) = favorites::open_folder(dir)
        && let Some(hwnd) = hwnd
    {
        tray::show_notice(
            hwnd,
            tray::NoticeKind::Warning,
            OPEN_FOLDER_FAILED_TITLE,
            &e,
        );
    }
}

/// メニューの1行。
#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    Command {
        id: usize,
        label: String,
    },
    /// 押せない説明の行。
    Disabled(String),
    Separator,
}

/// 項目名。`index` は 0 から。1〜9 番目だけ数字キーで選べる。名前の `&` はそのまま見せる。
pub fn label(index: usize, name: &str) -> String {
    let name = name.replace('&', "&&");
    if index < 9 {
        format!("&{}  {}", index + 1, name)
    } else {
        format!("{}  {}", index + 1, name)
    }
}

/// メニューに並べる行。`with_add` はエディタのメニューだけ true（トレイには「追加」を出さない）。
pub fn items(listing: &Listing, with_add: bool) -> Vec<Item> {
    let mut out = Vec::new();
    if listing.items.is_empty() {
        out.push(Item::Disabled("（お気に入りはまだありません）".into()));
    }
    for (i, favorite) in listing.items.iter().enumerate() {
        out.push(Item::Command {
            id: CMD_FAVORITE_FIRST + i,
            label: label(i, &favorite.name),
        });
    }
    if listing.broken > 0 {
        out.push(Item::Disabled(format!(
            "読み込めないファイルが {} 個あります",
            listing.broken
        )));
    }
    if listing.over_limit > 0 {
        out.push(Item::Disabled(format!(
            "上限の {} 本を超えた {} 個は出していません",
            MAX_ITEMS, listing.over_limit
        )));
    }
    out.push(Item::Separator);
    if with_add {
        out.push(Item::Command {
            id: CMD_ADD,
            label: "今の内容をお気に入りに追加".into(),
        });
    }
    out.push(Item::Command {
        id: CMD_ORGANIZE,
        label: "お気に入りを整理…".into(),
    });
    out
}

/// コマンド ID がお気に入りなら、その位置（0 から）。
pub fn favorite_index(id: usize) -> Option<usize> {
    (CMD_FAVORITE_FIRST..CMD_FAVORITE_FIRST + MAX_ITEMS)
        .contains(&id)
        .then(|| id - CMD_FAVORITE_FIRST)
}

/// 行からポップアップメニューを作る。作れなければ None。
/// 破棄は呼ぶ側。サブメニューとして親に付けたら、親の `DestroyMenu` で一緒に消える。
pub fn build(items: &[Item]) -> Option<HMENU> {
    // SAFETY: 途中で失敗したら、作りかけのメニューをここで破棄する。
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        if !append(menu, items) {
            let _ = DestroyMenu(menu);
            return None;
        }
        Some(menu)
    }
}

/// 今あるメニューの項目をすべて消し、items で作り直す（編集ウィンドウのメニューバーを開くたびに使う）。
/// 途中で失敗したら false（それまでに足した項目は残る）。
pub fn refill(menu: HMENU, items: &[Item]) -> bool {
    // SAFETY: menu は呼ぶ側が持っている有効なメニュー。項目は先頭から順に消す。
    unsafe {
        while GetMenuItemCount(Some(menu)) > 0 {
            if DeleteMenu(menu, 0, MF_BYPOSITION).is_err() {
                return false;
            }
        }
    }
    append(menu, items)
}

fn append(menu: HMENU, items: &[Item]) -> bool {
    // SAFETY: ラベルは to_wide_nul で作った 0 終端の文字列で、AppendMenuW が呼び出しの間だけ読む（中で複製される）。
    // 区切り線だけは null でよい。
    items.iter().all(|item| unsafe {
        match item {
            Item::Command { id, label } => {
                let wide = to_wide_nul(label);
                AppendMenuW(menu, MF_STRING, *id, PCWSTR(wide.as_ptr()))
            }
            Item::Disabled(label) => {
                let wide = to_wide_nul(label);
                AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(wide.as_ptr()))
            }
            Item::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
        }
        .is_ok()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::favorites::Favorite;

    fn listing(names: &[&str], broken: usize, over_limit: usize) -> Listing {
        Listing {
            items: names
                .iter()
                .map(|n| Favorite {
                    name: n.to_string(),
                    text: n.to_string(),
                })
                .collect(),
            broken,
            over_limit,
        }
    }

    #[test]
    fn label_has_access_key_only_for_first_nine() {
        assert_eq!(label(0, "要約"), "&1  要約");
        assert_eq!(label(8, "x"), "&9  x");
        assert_eq!(label(9, "x"), "10  x");
    }

    #[test]
    fn ampersand_in_name_is_shown_as_is() {
        assert_eq!(label(0, "Q&A"), "&1  Q&&A");
    }

    #[test]
    fn editor_menu_lists_favorites_then_add_and_organize() {
        assert_eq!(
            items(&listing(&["a", "b"], 0, 0), true),
            vec![
                Item::Command {
                    id: 100,
                    label: "&1  a".into()
                },
                Item::Command {
                    id: 101,
                    label: "&2  b".into()
                },
                Item::Separator,
                Item::Command {
                    id: CMD_ADD,
                    label: "今の内容をお気に入りに追加".into()
                },
                Item::Command {
                    id: CMD_ORGANIZE,
                    label: "お気に入りを整理…".into()
                },
            ]
        );
    }

    #[test]
    fn empty_listing_says_so_and_tray_menu_has_no_add() {
        assert_eq!(
            items(&Listing::default(), false),
            vec![
                Item::Disabled("（お気に入りはまだありません）".into()),
                Item::Separator,
                Item::Command {
                    id: CMD_ORGANIZE,
                    label: "お気に入りを整理…".into()
                },
            ]
        );
    }

    #[test]
    fn problems_are_shown_above_the_separator() {
        let got = items(&listing(&["a"], 2, 3), false);
        assert_eq!(
            got[1],
            Item::Disabled("読み込めないファイルが 2 個あります".into())
        );
        assert_eq!(
            got[2],
            Item::Disabled("上限の 20 本を超えた 3 個は出していません".into())
        );
        assert_eq!(got[3], Item::Separator);
    }

    #[test]
    fn command_ids_match_the_released_values() {
        // 配布済みの版と ID を変えない
        assert_eq!(CMD_ADD, 120);
        assert_eq!(CMD_ORGANIZE, 121);
    }

    #[test]
    fn favorite_index_maps_only_the_favorite_ids() {
        assert_eq!(favorite_index(100), Some(0));
        assert_eq!(favorite_index(119), Some(19));
        assert_eq!(favorite_index(99), None);
        assert_eq!(favorite_index(CMD_ADD), None);
        assert_eq!(favorite_index(1), None, "トレイの「開く」の ID");
    }

    /// メニューの pos 番目の項目の文字列。
    pub(crate) fn menu_text(menu: HMENU, pos: u32) -> String {
        use windows::Win32::UI::WindowsAndMessaging::{GetMenuStringW, MF_BYPOSITION};
        let mut buf = [0u16; 256];
        // SAFETY: menu は有効なメニューで、buf の長さまでしか書かれない。
        let len = unsafe { GetMenuStringW(menu, pos, Some(&mut buf), MF_BYPOSITION) };
        String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or_default()])
    }

    fn count(menu: HMENU) -> i32 {
        use windows::Win32::UI::WindowsAndMessaging::GetMenuItemCount;
        // SAFETY: menu は有効なメニュー。
        unsafe { GetMenuItemCount(Some(menu)) }
    }

    #[test]
    fn refill_replaces_previous_items() {
        let menu = build(&items(&listing(&["a", "b"], 0, 0), true)).unwrap();
        assert_eq!(count(menu), 5);
        assert!(refill(menu, &items(&listing(&["c"], 0, 0), true)));
        assert_eq!(count(menu), 4, "前の項目を残さない");
        assert_eq!(menu_text(menu, 0), "&1  c");
        // SAFETY: 今作ったメニューを、使い終わってから破棄する。
        unsafe {
            let _ = DestroyMenu(menu);
        }
    }
}
