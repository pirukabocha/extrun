/*!
入力プロンプト `$?{...}` のダイアログ

Win32 に「入力欄つきのダイアログを 1 行で出す」API は無い（`MessageBoxW` の
入力版は存在しない）。ダイアログテンプレートをメモリ上に組み立てて
`DialogBoxIndirectParamW` に渡すのが、余計なウィンドウ管理をせずに
Enter / Esc / タブ移動が正しく効く唯一の方法。

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
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// 組み込みコントロールのクラス atom
const ATOM_BUTTON: u16 = 0x0080;
const ATOM_EDIT: u16 = 0x0081;
const ATOM_STATIC: u16 = 0x0082;

/// `windows-sys` が `Win32_UI_Controls` を有効にしないと出さない定数
///
/// この 2 つのためだけにフィーチャーを増やすと、使わない API の定義まで抱える。
const SS_LEFT: u32 = 0x0000_0000;
const EM_SETSEL: u32 = 0x00B1;

/// ウィンドウスタイル
///
/// `windows-sys` では `WS_*` が `u32` で `DS_*` / `ES_*` / `BS_*` が `i32` なので、
/// ここで揃えておく。
const STYLE_DIALOG: u32 =
    (DS_MODALFRAME | DS_SETFONT | DS_CENTER) as u32 | WS_POPUP | WS_CAPTION | WS_SYSMENU;
const STYLE_STATIC: u32 = WS_CHILD | WS_VISIBLE | SS_LEFT;
const STYLE_EDIT: u32 = WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32;
const STYLE_OK: u32 = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32;
const STYLE_CANCEL: u32 = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32;

/// 入力欄のコントロール ID（`IDOK` = 1 / `IDCANCEL` = 2 と重ならない値）
const ID_EDIT: u16 = 100;

/// ダイアログの大きさ（ダイアログ単位。フォントに合わせて拡大縮小される）
const DIALOG_WIDTH: i16 = 240;
const DIALOG_HEIGHT: i16 = 84;
const MARGIN: i16 = 8;
const BUTTON_WIDTH: i16 = 50;
const BUTTON_HEIGHT: i16 = 14;

/// ダイアログとやり取りする値
struct PromptData {
    /// 入力欄の初期値
    default_value: Vec<u16>,
    /// OK が押されたときの入力内容
    result: Option<String>,
}

/// 入力ダイアログを出す
///
/// キャンセルまたはダイアログの生成失敗なら `None`。**呼び出し側は `None` を
/// 「実行しない」と解釈する**（失敗したまま既定値で走らせると、意図しない引数で
/// コマンドが動く）。
pub fn ask(message: &str, default_value: &str) -> Option<String> {
    let template = build_template(message);
    let mut data = PromptData {
        default_value: to_wide(default_value),
        result: None,
    };

    let selected = unsafe {
        DialogBoxIndirectParamW(
            GetModuleHandleW(null_mut()),
            template.as_ptr() as *const DLGTEMPLATE,
            null_mut(),
            Some(dialog_proc),
            &mut data as *mut PromptData as LPARAM,
        )
    };

    // テンプレートの組み立てを誤ると -1 が返る。黙って None にすると
    // 「選んだのに何も起きない」になって原因が追えないので、理由を見せる
    if selected == -1 {
        crate::show_error_dialog(
            "エラー",
            &format!("入力欄を表示できませんでした:\n{}", message),
        );
        return None;
    }

    if selected == IDOK as isize {
        data.result
    } else {
        None
    }
}

/// ダイアログテンプレートを組み立てる
///
/// 戻り値を `Vec<u32>` にしているのは、テンプレートの先頭が DWORD 境界に
/// なければならないため（`Vec<u16>` では 2 バイト境界しか保証されない）。
fn build_template(message: &str) -> Vec<u32> {
    let mut words: Vec<u16> = Vec::new();

    let button_y = DIALOG_HEIGHT - MARGIN - BUTTON_HEIGHT;
    let content_width = DIALOG_WIDTH - MARGIN * 2;

    // --- DLGTEMPLATE ---
    push_u32(&mut words, STYLE_DIALOG);
    push_u32(&mut words, 0); // 拡張スタイル
    words.push(4); // 項目数
    push_i16(&mut words, 0); // x（DS_CENTER で無視される）
    push_i16(&mut words, 0); // y
    push_i16(&mut words, DIALOG_WIDTH);
    push_i16(&mut words, DIALOG_HEIGHT);
    words.push(0); // メニューなし
    words.push(0); // 既定のウィンドウクラス
    push_str(&mut words, "ExtRun");
    words.push(9); // フォントの大きさ
    push_str(&mut words, "MS Shell Dlg");

    // --- 説明文 ---
    push_item(
        &mut words,
        STYLE_STATIC,
        MARGIN,
        MARGIN,
        content_width,
        34,
        u16::MAX, // 参照しないので ID は不要
        ATOM_STATIC,
        message,
    );

    // --- 入力欄 ---
    push_item(
        &mut words,
        STYLE_EDIT,
        MARGIN,
        MARGIN + 38,
        content_width,
        14,
        ID_EDIT,
        ATOM_EDIT,
        "",
    );

    // --- OK / キャンセル ---
    push_item(
        &mut words,
        STYLE_OK,
        DIALOG_WIDTH - MARGIN - BUTTON_WIDTH * 2 - 4,
        button_y,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
        IDOK as u16,
        ATOM_BUTTON,
        "OK",
    );
    push_item(
        &mut words,
        STYLE_CANCEL,
        DIALOG_WIDTH - MARGIN - BUTTON_WIDTH,
        button_y,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
        IDCANCEL as u16,
        ATOM_BUTTON,
        "キャンセル",
    );

    to_dword_buffer(&words)
}

/// 1 項目ぶんの `DLGITEMTEMPLATE` を書き込む
#[allow(clippy::too_many_arguments)]
fn push_item(
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

fn push_u32(words: &mut Vec<u16>, value: u32) {
    words.push(value as u16);
    words.push((value >> 16) as u16);
}

fn push_i16(words: &mut Vec<u16>, value: i16) {
    words.push(value as u16);
}

/// NUL 終端の UTF-16 文字列を書き込む
fn push_str(words: &mut Vec<u16>, text: &str) {
    words.extend(text.encode_utf16());
    words.push(0);
}

/// 次の DWORD 境界まで詰める
fn align_dword(words: &mut Vec<u16>) {
    if words.len() % 2 != 0 {
        words.push(0);
    }
}

/// `u16` の並びを DWORD 境界に載せ替える
fn to_dword_buffer(words: &[u16]) -> Vec<u32> {
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

fn to_wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(once(0)).collect()
}

/// ダイアログプロシージャ
///
/// `PromptData` へのポインタは `DialogBoxIndirectParamW` の引数で届く。
/// `WM_INITDIALOG` の時点で `GWLP_USERDATA` に控え、`WM_COMMAND` で取り出す。
unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_INITDIALOG => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam);

            let data = &*(lparam as *const PromptData);
            SetDlgItemTextW(hwnd, ID_EDIT as i32, data.default_value.as_ptr());
            // 既定値をすべて選択しておく。そのまま打てば置き換わり、
            // Enter だけなら既定値がそのまま使われる
            SendDlgItemMessageW(hwnd, ID_EDIT as i32, EM_SETSEL, 0, -1);

            1 // 最初のタブストップ（入力欄）にフォーカスを置く
        }

        WM_COMMAND => {
            let control = (wparam & 0xFFFF) as i32;
            if control != IDOK && control != IDCANCEL {
                return 0;
            }

            if control == IDOK {
                let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PromptData;
                if !data.is_null() {
                    (*data).result = Some(read_edit_text(hwnd));
                }
            }

            EndDialog(hwnd, control as isize);
            1
        }

        _ => 0,
    }
}

/// 入力欄の内容を読み取る
unsafe fn read_edit_text(hwnd: HWND) -> String {
    let edit = GetDlgItem(hwnd, ID_EDIT as i32);
    if edit.is_null() {
        return String::new();
    }

    // NUL の分を足して確保する
    let length = GetWindowTextLengthW(edit);
    if length <= 0 {
        return String::new();
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = GetWindowTextW(edit, buffer.as_mut_ptr(), buffer.len() as i32);

    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

// =====================================================================
// 書式（`$?{メッセージ=既定値}`）
// =====================================================================

/// `$?{...}` の中身を「メッセージ」と「既定値」に分ける
///
/// 最初の `=` で分ける。`=` が無ければ既定値なし。メッセージ側に `=` を
/// 含めたいときは、既定値を空にして書く余地を残していない（`$?{a=b=c}` は
/// メッセージ `a` / 既定値 `b=c` になる）。
pub fn split_spec(spec: &str) -> (&str, &str) {
    match spec.split_once('=') {
        Some((message, default_value)) => (message.trim_end(), default_value.trim_start()),
        None => (spec, ""),
    }
}

/// `$?{` を閉じる `}` の位置を返す
///
/// 中に `$t{...}` を書けるように深さを数える（`$?{新しい名前=$a_$t{yyyyMMdd}}`）。
/// 日時の書式に `{` は書けないので、数え間違えることはない。
pub fn find_close(bytes: &[u8]) -> Option<usize> {
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some(index),
            b'}' => depth -= 1,
            _ => {}
        }
    }

    None
}

/// 文字列に含まれる `$?{...}` を検証し、最初に見つかった問題を返す
///
/// 引数と作業フォルダはエスケープを解決する前に渡ってくるので、`^$` は飛ばす。
pub fn validate(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'^' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }

        if bytes[i] == b'$' && bytes[i + 1..].starts_with(b"?{") {
            let start = i + 3;
            let Some(end) = find_close(&bytes[start..]) else {
                return Some("$?{ が } で閉じられていません".to_string());
            };

            let spec = &text[start..start + end];
            let (message, _) = split_spec(spec);

            if message.trim().is_empty() {
                return Some(
                    "$?{} に説明がありません（例: $?{長辺のピクセル数=1280}）".to_string(),
                );
            }

            // 入れ子は先に解決する順序が決められないので断る。
            // 説明の中のパスや日時のプレースホルダーは解決される
            if spec.contains("$?{") {
                return Some("$?{} を入れ子にはできません".to_string());
            }

            i = start + end + 1;
            continue;
        }

        i += 1;
    }

    None
}

/// 文字列に含まれる `$?{...}` の中身を、書かれている順に取り出す
///
/// `^$` で打ち消したものは含めない。同じ内容が複数あってもそのまま並ぶので、
/// 重複の除去は呼び出し側（`RunContext`）が担う。
pub fn specs(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'^' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }

        if bytes[i] == b'$' && bytes[i + 1..].starts_with(b"?{") {
            let start = i + 3;
            match find_close(&bytes[start..]) {
                Some(end) => {
                    found.push(&text[start..start + end]);
                    i = start + end + 1;
                }
                // 閉じ忘れはパース時にエラーにしているので、ここに来るのは
                // 検証を通していないとき。プレースホルダーとして扱わない
                None => i += 1,
            }
            continue;
        }

        i += 1;
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実際にダイアログを出して確かめる
    ///
    /// テンプレートの組み立てはコンパイラが検査できないので、実機で 1 度は
    /// 通しておきたい。ただし画面が要るうえに 1 秒ほどかかるため、既定では
    /// 走らせない。**`cargo test -- --ignored` で実行する。**
    #[test]
    #[ignore = "画面が必要（cargo test -- --ignored で実行）"]
    fn ダイアログが出て入力を返す() {
        // ダイアログが立ち上がったら OK を押す
        std::thread::spawn(|| {
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let hwnd = unsafe { FindWindowW(null_mut(), to_wide("ExtRun").as_ptr()) };
                if !hwnd.is_null() {
                    unsafe { PostMessageW(hwnd, WM_COMMAND, IDOK as WPARAM, 0) };
                    return;
                }
            }
        });

        // 既定値は選択された状態で入っているので、そのまま OK すれば返ってくる
        assert_eq!(
            ask("テスト用の入力欄", "既定値"),
            Some("既定値".to_string())
        );
    }

    #[test]
    fn 既定値を分けて読める() {
        assert_eq!(
            split_spec("長辺のピクセル数=1280"),
            ("長辺のピクセル数", "1280")
        );
        assert_eq!(split_spec("新しい名前"), ("新しい名前", ""));
        // 空の既定値も書ける
        assert_eq!(split_spec("説明="), ("説明", ""));
    }

    /// `=` の前後の空白は落とす（`$?{幅 = 1280}` と書けるように）
    #[test]
    fn 等号のまわりの空白を落とす() {
        assert_eq!(split_spec("幅 = 1280"), ("幅", "1280"));
    }

    /// 2 つめ以降の `=` は既定値の一部
    #[test]
    fn 二つめの等号は既定値に含まれる() {
        assert_eq!(split_spec("式=a=b"), ("式", "a=b"));
    }

    #[test]
    fn 書かれた順に取り出せる() {
        assert_eq!(specs("-w $?{幅=1280} -h $?{高さ}"), vec!["幅=1280", "高さ"]);
        assert_eq!(specs("$p"), Vec::<&str>::new());
    }

    /// 中に `$t{...}` を書いても、最初の `}` で切れてしまわない
    #[test]
    fn 中の日時を巻き込んで終端を数える() {
        assert_eq!(
            specs("$?{新しい名前=$a_$t{yyyyMMdd}}"),
            vec!["新しい名前=$a_$t{yyyyMMdd}"]
        );
        assert_eq!(
            split_spec("新しい名前=$a_$t{yyyyMMdd}"),
            ("新しい名前", "$a_$t{yyyyMMdd}")
        );
        assert_eq!(validate("$?{新しい名前=$a_$t{yyyyMMdd}}"), None);
        // 後ろに続く指定も取りこぼさない
        assert_eq!(
            specs("$?{名前=$t{yyyy}} $?{幅}"),
            vec!["名前=$t{yyyy}", "幅"]
        );
    }

    #[test]
    fn エスケープしたものは取り出さない() {
        assert_eq!(specs("^$?{これは入力欄ではない}"), Vec::<&str>::new());
    }

    #[test]
    fn 正しい書き方は問題なしになる() {
        for text in [
            "$?{長辺のピクセル数=1280}",
            "$?{新しい名前=$a}",
            "-o $?{出力先=$d} $p",
            // 中括弧が続かない $? はそのまま通る
            "$?",
            "^$?{打ち消し}",
        ] {
            assert_eq!(validate(text), None, "{}", text);
        }
    }

    #[test]
    fn 閉じ忘れを検出する() {
        let message = validate("$?{幅").expect("エラーになる");
        assert!(message.contains("閉じられていません"), "{}", message);
    }

    #[test]
    fn 説明のない入力欄を検出する() {
        for text in ["$?{}", "$?{=1280}", "$?{   }"] {
            let message = validate(text).unwrap_or_else(|| panic!("{} はエラーになる", text));
            assert!(
                message.contains("説明がありません"),
                "{} → {}",
                text,
                message
            );
        }
    }

    #[test]
    fn 入れ子を断る() {
        let message = validate("$?{$?{入れ子}=1}").expect("エラーになる");
        assert!(message.contains("入れ子"), "{}", message);
    }
}
