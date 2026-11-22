//! 埋め込んだアイコン（assets/clipedit.ico）の読み込み。

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, HICON, LR_DEFAULTCOLOR};
use windows::core::Error;

/// tools/make-icon.ps1 で作ったアイコン（PNG 形式の .ico）。
const ICON_DATA: &[u8] = include_bytes!("../assets/clipedit.ico");

/// .ico の中から size に合う画像を選び、(データの位置, 長さ) を返す。
/// 同じ大きさが無ければ次に大きいもの、それも無ければいちばん大きいもの。
fn pick_icon_image(data: &[u8], size: i32) -> Option<(usize, usize)> {
    let read_u16 = |at: usize| Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let read_u32 = |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let count = read_u16(4)? as usize;
    let mut best: Option<(i32, usize, usize)> = None;
    for i in 0..count {
        let entry = 6 + 16 * i;
        let width = match *data.get(entry)? {
            0 => 256,
            w => i32::from(w),
        };
        let len = read_u32(entry + 8)? as usize;
        let offset = read_u32(entry + 12)? as usize;
        data.get(offset..offset + len)?;
        let better = match best {
            None => true,
            Some((b, _, _)) if b < size => width > b,
            Some((b, _, _)) => width >= size && width < b,
        };
        if better {
            best = Some((width, offset, len));
        }
    }
    best.map(|(_, offset, len)| (offset, len))
}

/// 埋め込んだアイコンを size × size で読み込む。
pub fn load_app_icon(size: i32) -> windows::core::Result<HICON> {
    let (offset, len) = pick_icon_image(ICON_DATA, size).ok_or_else(|| Error::from(E_FAIL))?;
    // SAFETY: pick_icon_image は ICON_DATA の範囲内だけを返す（範囲外ならスライスがパニックする）。
    // CreateIconFromResourceEx は渡したスライスを読むだけで、作ったアイコンは呼び出し側が破棄する。
    unsafe {
        CreateIconFromResourceEx(
            &ICON_DATA[offset..offset + len],
            true,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
}

/// アイコンの画像の幅（テストで、作ったアイコンの大きさを確かめる）。
#[cfg(test)]
pub fn icon_width(icon: HICON) -> i32 {
    use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};
    // SAFETY: icon は有効なアイコン。bitmap には BITMAP の大きさを渡す。
    // GetIconInfo が作ったビットマップ2つは、ここで破棄する（アイコンは破棄しない）。
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &raw mut info).unwrap();
        let mut bitmap = BITMAP::default();
        GetObjectW(
            info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&raw mut bitmap).cast()),
        );
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
        bitmap.bmWidth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    fn entry_width(size: i32) -> u8 {
        let (offset, _) = pick_icon_image(ICON_DATA, size).unwrap();
        // PNG の IHDR にある幅（16〜19 バイト目、ビッグエンディアン）
        ICON_DATA[offset + 19]
    }

    #[test]
    fn picks_exact_size_when_available() {
        assert_eq!(entry_width(16), 16);
        assert_eq!(entry_width(24), 24);
        assert_eq!(entry_width(40), 40);
    }

    #[test]
    fn picks_next_larger_size_otherwise() {
        assert_eq!(entry_width(18), 20);
        assert_eq!(entry_width(30), 32);
    }

    #[test]
    fn picks_largest_when_request_exceeds_all() {
        assert_eq!(entry_width(64), 48);
    }

    #[test]
    fn width_zero_means_256_pixels() {
        // .ico の項目の幅 0 は 256px を表す。ICONDIR（6 バイト）＋項目1つ（16 バイト）＋データ4バイト
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&4u32.to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend_from_slice(&[1, 2, 3, 4]);
        // 256px が欲しいとき、ちょうど同じ大きさとして選ばれる（48px などより優先される）
        let mut two = ico.clone();
        two[4] = 2;
        two.splice(22..22, [48, 48, 0, 0, 1, 0, 32, 0, 4, 0, 0, 0, 42, 0, 0, 0]);
        two[18] = 38; // 1つ目の項目のデータ位置を、2つ目の項目の後ろへずらす
        two.extend_from_slice(&[5, 6, 7, 8]);
        assert_eq!(pick_icon_image(&ico, 256), Some((22, 4)));
        assert_eq!(pick_icon_image(&two, 256), Some((38, 4)));
        assert_eq!(pick_icon_image(&two, 48), Some((42, 4)));
    }

    #[test]
    fn rejects_broken_icon_data() {
        assert_eq!(pick_icon_image(&[0, 0, 1], 16), None);
        assert_eq!(
            pick_icon_image(&[0, 0, 1, 0, 1, 0], 16),
            None,
            "項目が途中で切れている"
        );
    }

    #[test]
    fn loads_app_icon_at_requested_size() {
        for size in [16, 24, 32] {
            let icon = load_app_icon(size).unwrap();
            assert_eq!(icon_width(icon), size);
            // SAFETY: icon は今作った自分のアイコンで、このあと使わない。
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }
}
