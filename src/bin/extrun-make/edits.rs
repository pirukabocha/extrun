/*!
入力欄に Ctrl+A（全部を選ぶ）を効かせる

**Win32 の `EDIT` は Ctrl+A を自前では扱わない。** メモ帳やブラウザで効くのは、
それぞれのアプリが自分で拾っているから。素のダイアログでは、欄の中身を
一度に消したりコピーしたりする手立てが無い。

アクセラレータ（`TranslateAcceleratorW`）は自前のメッセージループが要るが、
このツールは `DialogBoxIndirectParamW` のモーダルループに乗っている。
そこで**入力欄の手続きを差し替えて**、Ctrl+A だけを横から取る。

差し替えは `GWLP_WNDPROC`（`comctl32` の `SetWindowSubclass` は使わない）。
`EDIT` クラスの手続きは全部の欄で同じものなので、覚えておく元の手続きは
1 つで足りる。

**どの欄かを決め打ちしない。** ダイアログの子を辿ってクラス名が `Edit` の
ものを全部差し替えるので、欄を足したときに付け忘れることがない。
読み取り専用の欄（作成した設定・プレビュー）にも効く — あそこは
「全部選んでコピー」がまさにやりたいことなので、外す理由が無い。
*/

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// 元の `EDIT` の手続き
///
/// クラスが同じなら手続きも同じなので、1 つ覚えておけば足りる。
static ORIGINAL_EDIT_PROC: AtomicIsize = AtomicIsize::new(0);

const EM_SETSEL: u32 = 0x00B1;
/// Ctrl+A が `WM_CHAR` で届くときの文字
const CTRL_A: usize = 0x01;

/// ダイアログの中の入力欄すべてに Ctrl+A を効かせる
///
/// `WM_INITDIALOG` で 1 回呼ぶ。
pub fn enable_select_all(dialog: HWND) {
    unsafe { EnumChildWindows(dialog, Some(hook_edit), 0) };
}

unsafe extern "system" fn hook_edit(child: HWND, _param: LPARAM) -> i32 {
    unsafe {
        let mut class = [0u16; 16];
        let len = GetClassNameW(child, class.as_mut_ptr(), class.len() as i32);
        let name = String::from_utf16_lossy(&class[..len.max(0) as usize]);

        // コンボボックスの中の入力欄まで拾わないよう、クラス名だけで見る
        if name.eq_ignore_ascii_case("Edit") {
            let original = SetWindowLongPtrW(child, GWLP_WNDPROC, edit_proc as *const () as isize);
            // 2 つ目以降は同じ値が返るので、上書きしても変わらない
            ORIGINAL_EDIT_PROC.store(original, Ordering::Relaxed);
        }

        1
    }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let original: WNDPROC = std::mem::transmute(ORIGINAL_EDIT_PROC.load(Ordering::Relaxed));

        match verdict(msg, wparam, ctrl_held()) {
            // 0 から -1 で「全部」
            Verdict::SelectAll => {
                SendMessageW(hwnd, EM_SETSEL, 0, -1);
                0
            }
            Verdict::Swallow => 0,
            Verdict::PassThrough => CallWindowProcW(original, hwnd, msg, wparam, lparam),
        }
    }
}

/// 届いたメッセージをどう扱うか
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// 全部選ぶ
    SelectAll,
    /// 握りつぶす
    Swallow,
    /// 元の手続きに渡す
    PassThrough,
}

/// **判定だけを切り出してある。** ウィンドウ手続きの中に埋めるとテストから
/// 触れなくなるので、`hwnd` を要らない形にして外に出した。
fn verdict(msg: u32, wparam: WPARAM, ctrl: bool) -> Verdict {
    match msg {
        WM_KEYDOWN if wparam == 'A' as usize && ctrl => Verdict::SelectAll,
        // 拾わずに通すと、扱えない制御文字として鳴る
        WM_CHAR if wparam == CTRL_A => Verdict::Swallow,
        _ => Verdict::PassThrough,
    }
}

fn ctrl_held() -> bool {
    // 最上位ビットが立っていれば押されている
    unsafe { GetKeyState(VK_CONTROL as i32) < 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_を押しながらの_a_で全部選ぶ() {
        assert_eq!(verdict(WM_KEYDOWN, 'A' as usize, true), Verdict::SelectAll);
    }

    /// Ctrl を押していなければただの文字入力
    #[test]
    fn ctrl_無しの_a_は素通し() {
        assert_eq!(
            verdict(WM_KEYDOWN, 'A' as usize, false),
            Verdict::PassThrough
        );
    }

    /// 通すと「扱えない制御文字」として鳴る
    #[test]
    fn ctrl_a_の文字は握りつぶす() {
        assert_eq!(verdict(WM_CHAR, CTRL_A, false), Verdict::Swallow);
    }

    #[test]
    fn ほかのキーは素通し() {
        assert_eq!(
            verdict(WM_KEYDOWN, 'B' as usize, true),
            Verdict::PassThrough
        );
        assert_eq!(verdict(WM_CHAR, 'a' as usize, false), Verdict::PassThrough);
        assert_eq!(verdict(WM_PAINT, 0, true), Verdict::PassThrough);
    }

    /// 差し込めているかは実物でしか分からない（ウィンドウが要る）
    #[test]
    #[ignore]
    fn 入力欄の手続きを差し替えられる() {
        use std::ptr::null_mut;
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

        unsafe {
            let parent = CreateWindowExW(
                0,
                extrun::dialog::to_wide("STATIC").as_ptr(),
                extrun::dialog::to_wide("親").as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                200,
                100,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null_mut()),
                null_mut(),
            );
            assert!(!parent.is_null());

            let edit = CreateWindowExW(
                0,
                extrun::dialog::to_wide("EDIT").as_ptr(),
                extrun::dialog::to_wide("").as_ptr(),
                WS_CHILD,
                0,
                0,
                100,
                20,
                parent,
                null_mut(),
                GetModuleHandleW(null_mut()),
                null_mut(),
            );
            assert!(!edit.is_null());

            let before = GetWindowLongPtrW(edit, GWLP_WNDPROC);
            enable_select_all(parent);
            let after = GetWindowLongPtrW(edit, GWLP_WNDPROC);

            assert_ne!(before, after, "入力欄の手続きが差し替わっていない");
            assert_eq!(
                ORIGINAL_EDIT_PROC.load(Ordering::Relaxed),
                before,
                "元の手続きを覚えていない"
            );

            DestroyWindow(parent);
        }
    }
}
