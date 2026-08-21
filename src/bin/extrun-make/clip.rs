/*!
クリップボードとファイル選択

**このツールが外の世界に触れるのはこの 2 つだけ。**
設定ファイルには書き戻さないので、作ったものはクリップボード経由で渡す。
*/

use extrun::dialog::to_wide;
use windows_sys::Win32::Foundation::{HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

const CF_UNICODETEXT: u32 = 13;

/// 文字列をクリップボードへ置く
///
/// `progress.rs` が中止したときに残りのパスを渡すのと同じ手順。
/// **置き換えるので、書き込みに成功してから空にする。**
pub fn copy(hwnd: HWND, text: &str) -> bool {
    let wide = to_wide(text);
    let bytes = std::mem::size_of_val(&wide[..]);

    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return false;
        }

        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            CloseClipboard();
            return false;
        }

        let buffer = GlobalLock(handle) as *mut u16;
        if buffer.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), buffer, wide.len());
        GlobalUnlock(handle);

        EmptyClipboard();
        let placed = !SetClipboardData(CF_UNICODETEXT, handle as HANDLE).is_null();
        CloseClipboard();

        placed
    }
}

/// 実行ファイルを選ばせる
///
/// 「コマンドラインに明るくない」への答えの半分がこれで、長いパスを手で
/// 打たずに済む。フォルダを選ぶ側（`:dir`）には付けていない —
/// あの欄には `$d` のようなプレースホルダーを書くことの方が多いため。
pub fn pick_executable(hwnd: HWND) -> Option<String> {
    pick(
        hwnd,
        "起動するアプリを選ぶ",
        "プログラム (*.exe;*.bat;*.cmd)\0*.exe;*.bat;*.cmd\0すべてのファイル\0*.*\0\0",
    )
}

/// アイコンの入ったファイルを選ばせる
pub fn pick_icon_source(hwnd: HWND) -> Option<String> {
    pick(
        hwnd,
        "アイコンの入ったファイルを選ぶ",
        "アイコンを持つファイル (*.ico;*.exe;*.dll)\0*.ico;*.exe;*.dll\0すべてのファイル\0*.*\0\0",
    )
}

/// 試す対象を選ばせる
///
/// **絞り込みを掛けない。** ⑤ は「このファイルを選んだらどうなるか」を見る
/// ためのもので、種類を限る理由が無い。
pub fn pick_any(hwnd: HWND) -> Option<String> {
    pick(hwnd, "試す対象を選ぶ", "すべてのファイル\0*.*\0\0")
}

fn pick(hwnd: HWND, title: &str, filter: &str) -> Option<String> {
    // 絞り込みは NUL 区切りで、最後に NUL を 2 つ置く決まり
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let title = to_wide(title);
    let mut file = vec![0u16; 1024];

    let mut options: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    options.hwndOwner = hwnd;
    options.lpstrFilter = filter.as_ptr();
    options.lpstrFile = file.as_mut_ptr();
    options.nMaxFile = file.len() as u32;
    options.lpstrTitle = title.as_ptr();
    options.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY;

    let picked = unsafe { GetOpenFileNameW(&mut options) };
    if picked == 0 {
        // 取り消したときも 0 が返る。理由を問わず「選ばなかった」でよい
        return None;
    }

    let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    Some(String::from_utf16_lossy(&file[..end]))
}
