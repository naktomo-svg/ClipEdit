//! 編集ウィンドウのメニューバー「お気に入り」。

use std::cell::RefCell;
use std::path::Path;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateMenu, CreatePopupMenu, DestroyMenu, HMENU, MF_POPUP, SetMenu,
};
use windows::core::w;

use super::{Editor, NoticeKind, PRIVATE, notice, notice_window};
use crate::favorite_menu;
use crate::favorites;

thread_local! {
    // メニューバーの「お気に入り」を開いたときに読んだ一覧。選ばれた項目はこれで処理する
    static MENU_BAR_LISTING: RefCell<favorites::Listing> = RefCell::new(favorites::Listing::default());
}

/// hwnd にメニューバーを付け、「お気に入り」のポップアップメニューを返す。
/// 中身は開く直前（`WM_INITMENUPOPUP`）に作る。
/// メニューはウィンドウに付けたので、ウィンドウと一緒に破棄される。途中で失敗したら、作ったメニューを破棄する。
pub(super) fn create_bar(hwnd: HWND) -> windows::core::Result<HMENU> {
    // SAFETY: 作ったばかりのメニューハンドルと、呼び出し元が作った hwnd を渡す。
    // 文字列は w! で作った 0 終端の定数。失敗したときに破棄するのは、まだウィンドウに付けていないメニューだけ。
    // ポップアップを bar に付けたあとは、bar を破棄すればポップアップも一緒に消える（監査 S38）。
    unsafe {
        let bar = CreateMenu()?;
        let bar_menu = match CreatePopupMenu() {
            Ok(menu) => menu,
            Err(e) => {
                let _ = DestroyMenu(bar);
                return Err(e);
            }
        };
        if let Err(e) = AppendMenuW(bar, MF_POPUP, bar_menu.0 as usize, w!("お気に入り(&F)")) {
            let _ = DestroyMenu(bar_menu);
            let _ = DestroyMenu(bar);
            return Err(e);
        }
        if let Err(e) = SetMenu(hwnd, Some(bar)) {
            let _ = DestroyMenu(bar);
            return Err(e);
        }
        Ok(bar_menu)
    }
}

impl Editor {
    /// 今の内容をお気に入りに追加し、通知の本文を返す。「残さない」印つきの内容は書かない。
    pub(super) fn add_favorite(&self, dir: &Path) -> Result<String, String> {
        if PRIVATE.with(|p| p.get()) {
            return Err(
                "パスワード管理ソフトなどがコピーした内容は、お気に入りに追加できません".into(),
            );
        }
        // 名前は本文の最初の行から作るので、通知（Windows の通知の履歴に残る）には出さない（監査 S35）
        let (n, _name) = favorites::add(dir, &self.text())?;
        Ok(format!("お気に入りの {n} 番に追加しました"))
    }

    /// メニューバーの「お気に入り」を開く直前に、フォルダを読み直して項目を作り直す。
    /// 読んだ一覧は、項目が選ばれたときのために覚えておく。
    pub(super) fn refresh_menu_bar(&self, dir: &Path) {
        let listing = favorites::list(dir);
        favorite_menu::refill(self.bar_menu, &favorite_menu::items(&listing, true));
        MENU_BAR_LISTING.with(|l| *l.borrow_mut() = listing);
    }

    /// メニューバーの「お気に入り」から選ばれた項目を、開いたときの一覧で処理する。
    pub(super) fn menu_bar_command(&self, dir: &Path, id: usize) {
        let listing = MENU_BAR_LISTING.with(|l| l.borrow().clone());
        self.run_favorite_command(dir, &listing, id);
    }

    /// メニューバーの「お気に入り」で選ばれた項目を処理する。
    fn run_favorite_command(&self, dir: &Path, listing: &favorites::Listing, id: usize) {
        if let Some(index) = favorite_menu::favorite_index(id) {
            if let Some(favorite) = listing.items.get(index) {
                self.replace_with(favorite);
            }
        } else if id == favorite_menu::CMD_ADD {
            match self.add_favorite(dir) {
                Ok(message) => notice(
                    NoticeKind::Info,
                    "ClipEdit: お気に入りに追加しました",
                    &message,
                ),
                Err(e) => notice(
                    NoticeKind::Warning,
                    "ClipEdit: お気に入りに追加できませんでした",
                    &e,
                ),
            }
        } else if id == favorite_menu::CMD_ORGANIZE {
            favorite_menu::organize(dir, notice_window());
        }
    }
}
