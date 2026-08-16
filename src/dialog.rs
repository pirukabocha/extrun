/*!
ダイアログテンプレートを組み立てる部品

Win32 には「入力欄や進行状況を持つダイアログを 1 行で出す」API が無い
（`MessageBoxW` にその種類は無い）。ダイアログテンプレートをメモリ上に
組み立てて `DialogBoxIndirectParamW` に渡すのが、余計なウィンドウ管理をせずに
Enter / Esc / タブ移動が正しく効く唯一の方法。

**組み立て方を知っているのはこのモジュールだけ**にする。`prompt.rs`（`$?{...}` の
入力欄）と `progress.rs`（起動の進行状況）が使う。境界の詰め方を誤ると
`DialogBoxIndirectParamW` が `-1` を返すだけで理由が分からないので、知識が
2 か所に分かれると片方だけ直す事故になる。

テンプレートの構造は次のとおり。**各項目は DWORD 境界から始める必要がある**ので、
`u16` 単位で組み立てて奇数個のところに詰め物を入れる。

```
DLGTEMPLATE      style / exstyle / 項目数 / x y cx cy
  0x0000         メニューなし
  0x0000         既定のウィンドウクラス
  "タイトル\0"
  9 "MS Shell Dlg\0"      DS_SETFONT を付けたときだけ
DLGITEMTEMPLATE  （DWORD 境界）style / exstyle / x y cx cy / ID
  0xFFFF 0x00xx  組み込みクラスの atom
  "文字列\0"
  0x0000         生成データなし
```
*/

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::LPARAM;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// 組み込みコントロールのクラス atom
pub const ATOM_BUTTON: u16 = 0x0080;
pub const ATOM_EDIT: u16 = 0x0081;
pub const ATOM_STATIC: u16 = 0x0082;

/// `windows-sys` が `Win32_UI_Controls` を有効にしないと出さない定数
///
/// これだけのためにフィーチャーを増やすと、使わない API の定義まで抱える。
pub const SS_LEFT: u32 = 0x0000_0000;

/// ウィンドウスタイル
///
/// `windows-sys` では `WS_*` が `u32` で `DS_*` / `ES_*` / `BS_*` が `i32` なので、
/// ここで揃えておく。
pub const STYLE_DIALOG: u32 =
    (DS_MODALFRAME | DS_SETFONT | DS_CENTER) as u32 | WS_POPUP | WS_CAPTION | WS_SYSMENU;
pub const STYLE_STATIC: u32 = WS_CHILD | WS_VISIBLE | SS_LEFT;
pub const STYLE_EDIT: u32 = WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32;
pub const STYLE_DEFAULT_BUTTON: u32 = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32;
pub const STYLE_BUTTON: u32 = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32;

/// 共通の余白とボタンの大きさ（ダイアログ単位。フォントに合わせて拡大縮小される）
///
/// 幅はダイアログごとに違ってよいが、この 3 つを揃えておくと ExtRun が出す
/// ダイアログの見た目が揃う。
pub const MARGIN: i16 = 8;
pub const BUTTON_WIDTH: i16 = 50;
pub const BUTTON_HEIGHT: i16 = 14;

/// ダイアログの題名
///
/// 一瞬で消えるダイアログはこれをそのまま使う。タスクバーに出るときの名前でも
/// あるので、何のウィンドウなのかが分かる必要がある。
pub const TITLE: &str = "ExtRun";

/// 題名に入れる項目名の上限（文字数）
///
/// タスクバーのボタンはこれよりずっと手前で切り詰められるので、長さの制限は
/// 見た目のためではなく、題名が際限なく伸びないようにするためのもの。
const MAX_TITLE_NAME: usize = 40;

/// 項目名を添えた題名を組み立てる（`項目名 ― ExtRun`）
///
/// **長く出したままになるダイアログだけがこれを使う**（進行状況）。ExtRun を
/// 2 つ動かしたときにタスクバーで見分けられないと困るのは、待っているあいだ
/// 残り続けるウィンドウだけだから。
///
/// 項目名を**先**に置くのは、タスクバーのボタンが右から切り詰められるため。
/// 区別に効くのは項目名の方で、「readme.txt - メモ帳」という Windows の
/// 並べ方とも揃う。
///
/// `name` は `MenuItem::name`（表示用の文字列）をそのまま渡してよい。
/// アクセスキーの `&` は含まれていない。
pub fn title_for(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return TITLE.to_string();
    }

    let mut shortened: String = name.chars().take(MAX_TITLE_NAME).collect();
    if shortened.chars().count() < name.chars().count() {
        shortened.push('…');
    }

    format!("{} ― {}", shortened, TITLE)
}

/// `DLGTEMPLATE` の見出しを書き込む
///
/// 項目の数は先に確定させておく必要がある（あとから数えて書き戻せない位置にある）。
pub fn push_header(words: &mut Vec<u16>, item_count: u16, width: i16, height: i16, title: &str) {
    push_u32(words, STYLE_DIALOG);
    push_u32(words, 0); // 拡張スタイル
    words.push(item_count);
    push_i16(words, 0); // x（DS_CENTER で無視される）
    push_i16(words, 0); // y
    push_i16(words, width);
    push_i16(words, height);
    words.push(0); // メニューなし
    words.push(0); // 既定のウィンドウクラス
    push_str(words, title);
    words.push(9); // フォントの大きさ
    push_str(words, "MS Shell Dlg");
}

/// 1 項目ぶんの `DLGITEMTEMPLATE` を書き込む
#[allow(clippy::too_many_arguments)]
pub fn push_item(
    words: &mut Vec<u16>,
    style: u32,
    x: i16,
    y: i16,
    cx: i16,
    cy: i16,
    id: u16,
    atom: u16,
    text: &str,
) {
    align_dword(words);

    push_u32(words, style);
    push_u32(words, 0); // 拡張スタイル
    push_i16(words, x);
    push_i16(words, y);
    push_i16(words, cx);
    push_i16(words, cy);
    words.push(id);

    words.push(0xFFFF); // 続くのがクラス名ではなく atom であることの目印
    words.push(atom);
    push_str(words, text);

    words.push(0); // 生成データなし
}

pub fn push_u32(words: &mut Vec<u16>, value: u32) {
    words.push(value as u16);
    words.push((value >> 16) as u16);
}

pub fn push_i16(words: &mut Vec<u16>, value: i16) {
    words.push(value as u16);
}

/// NUL 終端の UTF-16 文字列を書き込む
pub fn push_str(words: &mut Vec<u16>, text: &str) {
    words.extend(text.encode_utf16());
    words.push(0);
}

/// 次の DWORD 境界まで詰める
pub fn align_dword(words: &mut Vec<u16>) {
    if words.len() % 2 != 0 {
        words.push(0);
    }
}

/// `u16` の並びを DWORD 境界に載せ替える
///
/// 戻り値を `Vec<u32>` にしているのは、テンプレートの先頭が DWORD 境界に
/// なければならないため（`Vec<u16>` では 2 バイト境界しか保証されない）。
pub fn to_dword_buffer(words: &[u16]) -> Vec<u32> {
    let mut buffer = vec![0u32; words.len().div_ceil(2)];

    for (index, word) in words.iter().enumerate() {
        let slot = &mut buffer[index / 2];
        if index % 2 == 0 {
            *slot |= *word as u32;
        } else {
            *slot |= (*word as u32) << 16;
        }
    }

    buffer
}

pub fn to_wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(once(0)).collect()
}

/// テンプレートからモーダルダイアログを出す
///
/// 戻り値は `EndDialog` に渡された値。組み立てを誤ると `-1` が返るので、
/// 呼び出し側はそれを黙って捨てずに理由を見せること（「選んだのに何も
/// 起きない」になって原因が追えなくなる）。
pub fn show_modal(template: &[u32], proc: DLGPROC, data: LPARAM) -> isize {
    unsafe {
        DialogBoxIndirectParamW(
            GetModuleHandleW(null_mut()),
            template.as_ptr() as *const DLGTEMPLATE,
            null_mut(),
            proc,
            data,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 項目名が先。タスクバーのボタンは右から切り詰められるので、
    /// 区別に効く方を左に置かないと 2 つ並んだときに見分けられない
    #[test]
    fn 題名は項目名が先で_extrun_が後ろ() {
        assert_eq!(title_for("PNG に変換"), "PNG に変換 ― ExtRun");
    }

    /// 名前を持たない呼び出しでも「何のウィンドウか」は残す
    #[test]
    fn 名前が無ければ素の題名() {
        assert_eq!(title_for(""), "ExtRun");
        assert_eq!(title_for("   "), "ExtRun");
    }

    #[test]
    fn 長い項目名は丸める() {
        let 長い名前 = "あ".repeat(MAX_TITLE_NAME + 10);
        let 題名 = title_for(&長い名前);

        assert!(題名.starts_with(&"あ".repeat(MAX_TITLE_NAME)), "{}", 題名);
        assert!(題名.contains('…'), "{}", 題名);
        assert!(題名.ends_with(TITLE), "{}", 題名);
    }

    /// ちょうど上限の長さなら省略記号は付かない
    #[test]
    fn 上限ちょうどは丸めない() {
        let 名前 = "あ".repeat(MAX_TITLE_NAME);
        assert_eq!(title_for(&名前), format!("{} ― {}", 名前, TITLE));
    }
}
