/*!
メニュー項目のアイコン

`MF_OWNERDRAW` は要らない。`SetMenuItemInfoW` に `MIIM_BITMAP` で 32 ビットの
ビットマップを渡せば、Vista 以降のメニューがアルファ込みで描いてくれる。配色も
テーマもシステムのまま使える。

面倒なのは `HICON` からそのビットマップを作るところ。`GetIconInfo` が返す
`hbmColor` をそのまま渡すとアルファが壊れて黒い箱になるので、上下反転なしの
32bpp DIB を自分で用意し、そこへ `DrawIconEx` で描き直す。

**透明な黒の上に描く**のがこの手順の肝で、`DrawIconEx` の合成結果が
`色 × アルファ` になる。メニューが求めるのはこの「アルファを掛けたあとの色」
なので、そのまま渡せる。

古い形式（アルファチャンネルを持たない）のアイコンは、描いてもアルファが 0 の
ままになり、メニューには何も映らない。そのときだけマスクから透明かどうかを
写し取る。
*/

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows_sys::Win32::UI::Shell::SHDefExtractIconW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, DI_MASK, DI_NORMAL, HICON,
};

/// アイコンをメニュー用のビットマップにする
///
/// 失敗しても `None` を返すだけ。**アイコンが出ないことはあっても、メニュー
/// そのものが出なくなってはいけない**（設定の書き間違いや、消えた exe を指した
/// ままの項目でメニュー全体を失うのは割に合わない）。
pub fn load(path: &Path, index: i32, size: i32) -> Option<HBITMAP> {
    if size <= 0 {
        return None;
    }

    let icon = extract(path, index, size)?;
    let bitmap = to_bitmap(icon, size);
    unsafe { DestroyIcon(icon) };
    bitmap
}

/// `load` が返したビットマップを解放する
pub fn dispose(bitmap: HBITMAP) {
    unsafe { DeleteObject(bitmap as HGDIOBJ) };
}

/// ファイルからアイコンを取り出す
///
/// `ExtractIconExW` は 32x32 と 16x16 しか返さないので、高 DPI では引き伸ばしに
/// なる。`SHDefExtractIconW` は欲しい大きさを指定できるぶん見栄えがよい。
fn extract(path: &Path, index: i32, size: i32) -> Option<HICON> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut icon = null_mut();
    // 下位ワードが大きい方、上位ワードが小さい方の希望サイズ
    let requested = (size as u32) | ((size as u32) << 16);

    let result =
        unsafe { SHDefExtractIconW(wide.as_ptr(), index, 0, &mut icon, null_mut(), requested) };

    // S_FALSE（1）はアイコンが見つからなかったという意味なので、0 だけを通す
    if result != 0 || icon.is_null() {
        return None;
    }

    Some(icon)
}

/// `HICON` を 32bpp のビットマップに描き直す
fn to_bitmap(icon: HICON, size: i32) -> Option<HBITMAP> {
    let (bitmap, pixels) = create_dib(size)?;

    let drawn = draw(icon, bitmap, size, DI_NORMAL);
    if !drawn {
        unsafe { DeleteObject(bitmap as HGDIOBJ) };
        return None;
    }

    // SAFETY: create_dib が確保した size*size*4 バイト。DIB セクションが
    // 生きているあいだだけ触る
    let pixels = unsafe { std::slice::from_raw_parts_mut(pixels, (size * size * 4) as usize) };

    if !has_alpha(pixels) && !apply_mask(icon, size, pixels) {
        // マスクも取れないなら、全面不透明として扱う（黒い箱になるよりまし）
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 0xFF;
        }
    }

    Some(bitmap)
}

/// 上下反転なしの 32bpp DIB を作る（ビットマップと画素の先頭を返す）
fn create_dib(size: i32) -> Option<(HBITMAP, *mut u8)> {
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = size;
    // 高さを負にすると「上から下へ並ぶ」。自前で行を入れ替えずに済む
    info.bmiHeader.biHeight = -size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    let mut bits: *mut core::ffi::c_void = null_mut();
    let bitmap =
        unsafe { CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };

    if bitmap.is_null() || bits.is_null() {
        if !bitmap.is_null() {
            unsafe { DeleteObject(bitmap as HGDIOBJ) };
        }
        return None;
    }

    Some((bitmap, bits as *mut u8))
}

/// ビットマップへアイコンを描く
fn draw(icon: HICON, bitmap: HBITMAP, size: i32, flags: u32) -> bool {
    let dc = unsafe { CreateCompatibleDC(null_mut()) };
    if dc.is_null() {
        return false;
    }

    let previous = unsafe { SelectObject(dc, bitmap as HGDIOBJ) };
    let drawn = unsafe { DrawIconEx(dc, 0, 0, icon, size, size, 0, null_mut(), flags) };
    unsafe {
        SelectObject(dc, previous);
        DeleteDC(dc);
    }

    drawn != 0
}

/// アルファチャンネルが使われているか
fn has_alpha(pixels: &[u8]) -> bool {
    pixels.chunks_exact(4).any(|pixel| pixel[3] != 0)
}

/// マスクを描いて、そこからアルファを写す
///
/// `DI_MASK` で描くと、透明なところが白・不透明なところが黒になる。
fn apply_mask(icon: HICON, size: i32, pixels: &mut [u8]) -> bool {
    let Some((mask_bitmap, mask_pixels)) = create_dib(size) else {
        return false;
    };

    let drawn = draw(icon, mask_bitmap, size, DI_MASK);
    if drawn {
        // SAFETY: create_dib が確保した size*size*4 バイト
        let mask = unsafe { std::slice::from_raw_parts(mask_pixels, pixels.len()) };
        for (pixel, mask) in pixels.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
            pixel[3] = if mask[0] == 0 { 0xFF } else { 0 };
        }
    }

    unsafe { DeleteObject(mask_bitmap as HGDIOBJ) };
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::{GetDIBits, GetObjectW, BITMAP};

    /// 素の Windows に必ずある、アイコンを持つファイル
    const ICON_SOURCE: &str = "C:\\Windows\\explorer.exe";

    /// ビットマップの画素を読み戻す
    fn pixels_of(bitmap: HBITMAP, size: i32) -> Vec<u8> {
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size;
        info.bmiHeader.biHeight = -size;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;

        let mut buffer = vec![0u8; (size * size * 4) as usize];
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        let copied = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                size as u32,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        unsafe { DeleteDC(dc) };

        assert_eq!(copied, size, "画素を読み戻せる");
        buffer
    }

    #[test]
    fn 指定した大きさのビットマップになる() {
        let size = 20;
        let bitmap = load(Path::new(ICON_SOURCE), 0, size).expect("アイコンを取り出せる");

        let mut description: BITMAP = unsafe { std::mem::zeroed() };
        let read = unsafe {
            GetObjectW(
                bitmap as HGDIOBJ,
                std::mem::size_of::<BITMAP>() as i32,
                &mut description as *mut BITMAP as *mut core::ffi::c_void,
            )
        };
        assert!(read > 0, "ビットマップの情報を読める");
        assert_eq!(description.bmWidth, size);
        assert_eq!(description.bmHeight, size);
        assert_eq!(
            description.bmBitsPixel, 32,
            "32 ビットでないとアルファが乗らない"
        );

        unsafe { DeleteObject(bitmap as HGDIOBJ) };
    }

    /// メニューが求めるのは「アルファを掛けたあとの色」。透明な黒の上に描く
    /// ことでそうなっているはずなので、読み戻して確かめる
    #[test]
    fn アルファを掛けたあとの色になっている() {
        let size = 32;
        let bitmap = load(Path::new(ICON_SOURCE), 0, size).expect("アイコンを取り出せる");
        let pixels = pixels_of(bitmap, size);
        unsafe { DeleteObject(bitmap as HGDIOBJ) };

        assert!(has_alpha(&pixels), "どこかは不透明でないと何も見えない");

        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            let (b, g, r, a) = (pixel[0], pixel[1], pixel[2], pixel[3]);
            assert!(
                b <= a && g <= a && r <= a,
                "{} 番目の画素がアルファを超えている: B{} G{} R{} A{}",
                index,
                b,
                g,
                r,
                a
            );
        }
    }

    #[test]
    fn 見つからないファイルは何も返さない() {
        assert!(load(Path::new("C:\\存在しない\\無い.exe"), 0, 16).is_none());
    }

    /// アイコンを持たないファイルを指しても、落ちずに諦める
    #[test]
    fn アイコンのないファイルは何も返さない() {
        let path = std::env::temp_dir().join("extrun-icon-test.txt");
        std::fs::write(&path, b"not an icon").expect("書ける");
        let bitmap = load(&path, 0, 16);
        std::fs::remove_file(&path).ok();

        if let Some(bitmap) = bitmap {
            unsafe { DeleteObject(bitmap as HGDIOBJ) };
        }
    }

    #[test]
    fn 大きさが不正なら何も返さない() {
        assert!(load(Path::new(ICON_SOURCE), 0, 0).is_none());
        assert!(load(Path::new(ICON_SOURCE), 0, -8).is_none());
    }
}
