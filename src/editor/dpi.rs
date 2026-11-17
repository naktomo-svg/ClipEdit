use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject, FF_MODERN,
    FIXED_PITCH, FW_NORMAL, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, HICON, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, SendMessageW, WM_GETFONT,
    WM_SETFONT, WM_SETICON,
};
use windows::core::w;

const FONT_POINTS: i32 = 11;

/// 作業領域 work の中央に置く矩形 (x, y, 幅, 高さ)。作業領域より大きければ縮める。
pub(super) fn center_rect(work: RECT, width: i32, height: i32) -> (i32, i32, i32, i32) {
    // 作業領域が逆向きや極端な値でもあふれないよう i64 で計算し、大きさは 0 以上にする（監査 S16）
    let span = |from: i32, to: i32| (i64::from(to) - i64::from(from)).max(0);
    let (work_w, work_h) = (span(work.left, work.right), span(work.top, work.bottom));
    let w = i64::from(width).clamp(0, work_w);
    let h = i64::from(height).clamp(0, work_h);
    let x = i64::from(work.left) + (work_w - w) / 2;
    let y = i64::from(work.top) + (work_h - h) / 2;
    (to_i32(x), to_i32(y), to_i32(w), to_i32(h))
}

/// i32 に収まるように切り詰める（`center_rect` の結果は、作業領域の中なので本来は必ず収まる）。
fn to_i32(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or_default()
}

pub(super) fn scale(value: i32, dpi: u32) -> i32 {
    value * dpi as i32 / 96
}

pub(super) fn apply_font(edit: HWND, dpi: u32) {
    // SAFETY: フォント名は w! で作った 0 終端の文字列。WM_SETFONT の後は編集欄が新しいフォントを使うので、
    // 前のフォントを破棄する。ふだんは自分で作ったもの（最初はシステムのフォントで 0 が返る）。
    // ほかのプロセスが WM_SETFONT で自分のハンドルを送り込んでいた場合は、そのプロセスのハンドルなので
    // DeleteObject が失敗するだけで、こちらのフォントが1つ漏れる（監査 S15）。
    unsafe {
        let old = SendMessageW(edit, WM_GETFONT, None, None);
        let font = CreateFontW(
            -(FONT_POINTS * dpi as i32 / 72),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (FIXED_PITCH.0 | FF_MODERN.0) as u32,
            w!("BIZ UDゴシック"),
        );
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        if old.0 != 0 {
            let _ = DeleteObject(HGDIOBJ(HFONT(old.0 as *mut _).0));
        }
    }
}

/// タイトルバーとタスクバーのアイコンを、トレイと同じアイコンから dpi に合った大きさで作って付ける。
pub(super) fn apply_icons(hwnd: HWND, dpi: u32) {
    for (kind, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
        // SAFETY: 値だけを渡す。
        let size = unsafe { GetSystemMetricsForDpi(metric, dpi) };
        let Ok(icon) = crate::icon::load_app_icon(size) else {
            continue;
        };
        // SAFETY: icon は今作った自分のアイコン。WM_SETICON は前のアイコンを返すので、それを破棄する。
        // ふだんは自分が前に付けたもの（最初は 0）。ほかのプロセスが WM_SETICON で送り込んだものなら、
        // DestroyIcon が失敗するだけで、こちらのアイコンが1つ漏れる（監査 S15）。
        unsafe {
            let old = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(kind as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
            if old.0 != 0 {
                let _ = DestroyIcon(HICON(old.0 as *mut _));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_inside_work_area() {
        let work = RECT {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1040,
        };
        assert_eq!(center_rect(work, 800, 500), (2480, 270, 800, 500));
    }

    #[test]
    fn shrinks_to_small_work_area() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 640,
            bottom: 400,
        };
        assert_eq!(center_rect(work, 1200, 750), (0, 0, 640, 400));
    }

    // 監査 S16: 作業領域が極端な値・逆向きでも、あふれてパニックせず、大きさは 0 以上になる
    #[test]
    fn extreme_work_area_does_not_overflow() {
        let work = RECT {
            left: 1_167_600_539,
            top: i32::MIN,
            right: -979_883_110,
            bottom: i32::MAX,
        };
        let (_, _, w, h) = center_rect(work, 800, 500);
        assert!(w >= 0 && h >= 0, "幅 {w}・高さ {h}");
        let inverted = RECT {
            left: 100,
            top: 0,
            right: 0,
            bottom: 50,
        };
        assert_eq!(center_rect(inverted, 800, 500), (100, 0, 0, 50));
    }
}
