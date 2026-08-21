/*!
ダイアログ単位が何ピクセルになるかを、画面を作る前に測る

**なぜ先に測るのか。** 出力欄とプレビュー欄の高さは、画面に収まる範囲で
できるだけ大きく取りたい。ところがテンプレートを組み立てる時点ではまだ
ウィンドウが無く、`MapDialogRect` は使えない（あれはダイアログの内部データを
読むので、ウィンドウが要る）。

作ってから縮める手もあるが、そうすると**縮めた欄より下にあるものを全部
動かす**ことになり、開閉のたびに位置を計算し直す羽目になる。先に測って
組み立てれば、テンプレートは 1 回で正しい形になる。

測り方はダイアログマネージャと同じ。`DS_SETFONT` で `MS Shell Dlg` 9pt を
指定しているので、そのフォントの `tmHeight` が縦の基準単位になり、
**1 ダイアログ単位 = tmHeight / 8 ピクセル**。日本語環境では `MS Shell Dlg` が
MS UI Gothic に解決され、96 dpi で tmHeight = 12（1 dlu = 1.5 px）になる。
*/

use std::ptr::null_mut;

use extrun::dialog::to_wide;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateFontW, DEFAULT_CHARSET, DeleteDC, DeleteObject, GetDC,
    GetTextMetricsW, ReleaseDC, SelectObject, TEXTMETRICW,
};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

/// `dialog.rs` が `DS_SETFONT` に書いているフォント
const FONT_FACE: &str = "MS Shell Dlg";
const FONT_POINTS: i32 = 9;

/// 縦の基準単位（`tmHeight`）
///
/// 取れなかったときは 96 dpi の実測値に倒す。**0 を返さない**のが肝で、
/// 割り算の分母になるため。
fn base_y(dpi: u32) -> i32 {
    unsafe {
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);

        // MulDiv(ポイント数, dpi, 72) の符号を反転したものが「文字の高さ」
        let height = -((FONT_POINTS * dpi as i32 + 36) / 72);
        let face = to_wide(FONT_FACE);
        let font = CreateFontW(
            height,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            0,
            0,
            face.as_ptr(),
        );

        let old = SelectObject(dc, font);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        let ok = GetTextMetricsW(dc, &mut tm);

        SelectObject(dc, old);
        DeleteObject(font);
        DeleteDC(dc);
        ReleaseDC(null_mut(), screen);

        if ok != 0 && tm.tmHeight > 0 {
            tm.tmHeight
        } else {
            12
        }
    }
}

/// ダイアログの高さに使えるダイアログ単位の上限
///
/// タスクバーを除いた作業領域から、タイトルバーと枠のぶんを引いた残り。
/// **画面が取れなかったときは大きめの値を返す** — 縮めすぎて欄が潰れるより、
/// はみ出して動かせる方がまだ直せる。
pub fn available_height_dlu(style: u32, ex_style: u32) -> i16 {
    unsafe {
        let dpi = GetDpiForSystem().max(96);

        let mut work = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0) == 0 {
            return i16::MAX;
        }
        let available = work.bottom - work.top;

        // タイトルバーと枠の厚みは、実際のスタイルから出す
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        AdjustWindowRectExForDpi(&mut frame, style, 0, ex_style, dpi);
        let chrome = frame.bottom - frame.top;

        let usable = (available - chrome).max(0);
        ((usable * 8) / base_y(dpi)).clamp(0, i16::MAX as i32) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分母になるので 0 を返してはいけない
    #[test]
    fn 基準単位は必ず正の値() {
        for dpi in [96, 120, 144, 192] {
            assert!(base_y(dpi) > 0, "dpi {}", dpi);
        }
    }

    /// 96 dpi の日本語環境では MS UI Gothic に解決されて 12 になる。
    /// 拡大率に比例して増える（この比が dlu → px の換算そのもの）
    #[test]
    fn 基準単位は拡大率に比例する() {
        let 標準 = base_y(96);
        let 倍 = base_y(192);
        assert!(倍 > 標準, "{} → {}", 標準, 倍);
    }

    /// 実際の画面で測るので値は環境次第だが、常識的な範囲には入る
    #[test]
    fn 使える高さは画面から出る() {
        let 高さ = available_height_dlu(0x8000_0000, 0);
        assert!(高さ > 100, "{}", 高さ);
    }
}
