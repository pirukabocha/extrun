/*!
入力プロンプト `$?{...}` のダイアログ

テンプレートの組み立ては `dialog.rs` が担当する。ここが持つのは入力欄という
ダイアログの中身と、`$?{...}` の書式の解釈。
*/

use crate::dialog::{
    self, push_header, push_item, to_dword_buffer, to_wide, ATOM_BUTTON, ATOM_EDIT, ATOM_STATIC,
    BUTTON_HEIGHT, BUTTON_WIDTH, MARGIN, STYLE_BUTTON, STYLE_DEFAULT_BUTTON, STYLE_EDIT,
    STYLE_STATIC,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// `windows-sys` が `Win32_UI_Controls` を有効にしないと出さない定数
const EM_SETSEL: u32 = 0x00B1;

/// 入力欄のコントロール ID（`IDOK` = 1 / `IDCANCEL` = 2 と重ならない値）
const ID_EDIT: u16 = 100;

/// ダイアログの大きさ（ダイアログ単位。フォントに合わせて拡大縮小される）
const DIALOG_WIDTH: i16 = 240;
const MESSAGE_HEIGHT: i16 = 20;
const ERROR_HEIGHT: i16 = 20;
const EDIT_HEIGHT: i16 = 14;

/// ファイル名に使えない文字（Windows）
const FORBIDDEN_NAME_CHARS: &str = "\\/:*?\"<>|";

/// ファイル名 1 要素の長さの上限（NTFS。UTF-16 の符号単位で数える）
///
/// パス全体の上限（`MAX_PATH`）は、入力値がパスのどこに入るか分からないので
/// 判定できない。ここで見るのは名前 1 つぶんだけ。
const MAX_NAME_LENGTH: usize = 255;

/// 予約された装置名（拡張子を付けても使えない）
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 入力値に課す決まり（`$?` と `{` のあいだに書く）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// 制限なし（`$?{...}`）
    Any,
    /// 整数（`$?int{...}`）
    Int,
    /// 数値（`$?num{...}`）
    Num,
    /// ファイル名に使える文字（`$?name{...}`）
    Name,
}

impl Rule {
    /// `$?` と `{` のあいだの語を解釈する（未知の語は `None`）
    pub fn from_keyword(keyword: &str) -> Option<Rule> {
        match keyword {
            "" => Some(Rule::Any),
            "int" => Some(Rule::Int),
            "num" => Some(Rule::Num),
            "name" => Some(Rule::Name),
            _ => None,
        }
    }

    /// 入力された文字を整える
    ///
    /// 数値と名前では前後の空白が意味を持たない（むしろファイル名では
    /// 末尾の空白が不正）。制限なしのときは打たれたとおりに渡す。
    pub fn normalize<'a>(&self, value: &'a str) -> &'a str {
        match self {
            Rule::Any => value,
            _ => value.trim(),
        }
    }

    /// 決まりを満たしているか調べる（満たさなければ理由を返す）
    pub fn check(&self, value: &str) -> Result<(), String> {
        match self {
            Rule::Any => Ok(()),

            Rule::Int => match value.parse::<i64>() {
                Ok(_) => Ok(()),
                // 全角数字や単位付き（1280px）はここで弾かれる
                Err(_) => Err("整数を入力してください（例: 1280）".to_string()),
            },

            Rule::Num => match value.parse::<f64>() {
                Ok(number) if number.is_finite() => Ok(()),
                _ => Err("数値を入力してください（例: 1.5）".to_string()),
            },

            Rule::Name => check_file_name(value),
        }
    }
}

/// ファイル名として使えるか調べる
fn check_file_name(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("名前を入力してください".to_string());
    }

    if let Some(found) = value.chars().find(|c| FORBIDDEN_NAME_CHARS.contains(*c)) {
        return Err(format!(
            "ファイル名に使えない文字が入っています: {}（使えないのは {}）",
            found, FORBIDDEN_NAME_CHARS
        ));
    }

    if value.chars().any(|c| (c as u32) < 0x20) {
        return Err("ファイル名に使えない文字が入っています".to_string());
    }

    if value.ends_with('.') || value.ends_with(' ') {
        return Err("名前の終わりに . と空白は使えません".to_string());
    }

    // CON.txt のように拡張子を付けても予約名は使えない
    let stem = value.split('.').next().unwrap_or(value);
    if RESERVED_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return Err(format!("{} は Windows が予約している名前です", stem));
    }

    let length = value.encode_utf16().count();
    if length > MAX_NAME_LENGTH {
        return Err(format!(
            "名前が長すぎます（{} 文字。{} 文字まで）",
            length, MAX_NAME_LENGTH
        ));
    }

    Ok(())
}

/// ダイアログとやり取りする値
struct PromptData {
    /// 入力欄の初期値
    default_value: Vec<u16>,
    /// OK が押されたときの入力内容
    result: Option<String>,
}

/// 入力ダイアログを出す（決まりを満たすまで聞き直す）
///
/// キャンセルまたはダイアログの生成失敗なら `None`。**呼び出し側は `None` を
/// 「実行しない」と解釈する**（失敗したまま既定値で走らせると、意図しない引数で
/// コマンドが動く）。
///
/// 決まりを満たさないときに打ち切らないのは、それでは「キャンセルと同じで
/// 何も起きない」になり、検証がかえって邪魔になるため。打った内容を残した
/// まま理由を添えて出し直す。
pub fn ask(rule: Rule, message: &str, default_value: &str) -> Option<String> {
    let mut current = default_value.to_string();
    let mut problem: Option<String> = None;

    loop {
        let value = show_dialog(message, problem.as_deref(), &current)?;
        let value = rule.normalize(&value).to_string();

        match rule.check(&value) {
            Ok(()) => return Some(value),
            Err(reason) => {
                problem = Some(reason);
                current = value;
            }
        }
    }
}

/// ダイアログを 1 回出す
fn show_dialog(message: &str, problem: Option<&str>, default_value: &str) -> Option<String> {
    let template = build_template(message, problem);
    let mut data = PromptData {
        default_value: to_wide(default_value),
        result: None,
    };

    let selected = dialog::show_modal(
        &template,
        Some(dialog_proc),
        &mut data as *mut PromptData as LPARAM,
    );

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
fn build_template(message: &str, problem: Option<&str>) -> Vec<u32> {
    let mut words: Vec<u16> = Vec::new();

    // 理由の行があるときだけ縦に伸ばす。常に場所を空けておくと、
    // 何も問題がないときに間の抜けたダイアログになる
    let error_height = if problem.is_some() { ERROR_HEIGHT } else { 0 };
    let edit_y = MARGIN + MESSAGE_HEIGHT + error_height;
    let button_y = edit_y + EDIT_HEIGHT + MARGIN;
    let dialog_height = button_y + BUTTON_HEIGHT + MARGIN;
    let content_width = DIALOG_WIDTH - MARGIN * 2;

    // 入力欄は答えるまでのあいだしか出ていないので、題名は素の「ExtRun」でよい
    // （見分けが要るのは、待っているあいだ残り続ける進行状況ダイアログの方）
    push_header(
        &mut words,
        if problem.is_some() { 5 } else { 4 },
        DIALOG_WIDTH,
        dialog_height,
        dialog::TITLE,
    );

    // --- 説明文 ---
    push_item(
        &mut words,
        STYLE_STATIC,
        MARGIN,
        MARGIN,
        content_width,
        MESSAGE_HEIGHT,
        u16::MAX, // 参照しないので ID は不要
        ATOM_STATIC,
        message,
    );

    // --- 決まりを満たさなかった理由 ---
    if let Some(problem) = problem {
        push_item(
            &mut words,
            STYLE_STATIC,
            MARGIN,
            MARGIN + MESSAGE_HEIGHT,
            content_width,
            ERROR_HEIGHT,
            u16::MAX,
            ATOM_STATIC,
            problem,
        );
    }

    // --- 入力欄 ---
    push_item(
        &mut words,
        STYLE_EDIT,
        MARGIN,
        edit_y,
        content_width,
        EDIT_HEIGHT,
        ID_EDIT,
        ATOM_EDIT,
        "",
    );

    // --- OK / キャンセル ---
    push_item(
        &mut words,
        STYLE_DEFAULT_BUTTON,
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
        STYLE_BUTTON,
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

/// 書かれた 1 つの入力欄
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prompt<'a> {
    /// `$?int{長辺=1280}` 全体
    ///
    /// `RunContext` の見出しに使う。**決まりまで含めた文字列**でなければ、
    /// `$?{幅}` と `$?int{幅}` が同じ答えを共有してしまう。
    pub source: &'a str,
    pub rule: Rule,
    pub message: &'a str,
    pub default_value: &'a str,
}

/// `text` の `start` が入力欄の始まりなら、`{` までの長さを返す
///
/// `text::split_args` が中括弧を数えるのにも使う（`$?name{新しい 名前}` の
/// ように説明に空白が入るため、`{...}` の中では引数を区切ってはいけない）。
pub fn opening_len(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.get(start) != Some(&b'$') || bytes.get(start + 1) != Some(&b'?') {
        return None;
    }

    // `$?` と `{` のあいだの決まり（何も無ければ制限なし）
    let keyword_start = start + 2;
    let mut i = keyword_start;
    while bytes.get(i).is_some_and(|b| b.is_ascii_alphabetic()) {
        i += 1;
    }

    // `{` が続かなければ入力欄ではない（PowerShell の `$?` など）
    if bytes.get(i) != Some(&b'{') {
        return None;
    }

    Rule::from_keyword(&text[keyword_start..i])?;
    Some(i + 1 - start)
}

/// `text` の `start` から始まる `$?...{...}` を読む
///
/// 戻り値は読み取った内容と、`}` の次の位置。入力欄でなければ `None`。
pub fn parse_at(text: &str, start: usize) -> Option<(Prompt<'_>, usize)> {
    let bytes = text.as_bytes();
    let opening = opening_len(text, start)?;
    let rule = Rule::from_keyword(&text[start + 2..start + opening - 1])?;

    let inner_start = start + opening;
    let end = find_close(&bytes[inner_start..])?;
    let (message, default_value) = split_spec(&text[inner_start..inner_start + end]);

    Some((
        Prompt {
            source: &text[start..inner_start + end + 1],
            rule,
            message,
            default_value,
        },
        inner_start + end + 1,
    ))
}

/// 中括弧の中身を「説明」と「既定値」に分ける
///
/// 最初の `=` で分ける。`=` が無ければ既定値なし（`$?{a=b=c}` は説明 `a` /
/// 既定値 `b=c` になる）。
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

        if bytes[i] != b'$' || bytes.get(i + 1) != Some(&b'?') {
            i += 1;
            continue;
        }

        // `$?` に続く語と `{` を自分で読む。`parse_at` は入力欄でないものを
        // まとめて `None` で返すので、理由を言い分けるにはここで見る必要がある
        let keyword_start = i + 2;
        let mut cursor = keyword_start;
        while bytes.get(cursor).is_some_and(|b| b.is_ascii_alphabetic()) {
            cursor += 1;
        }

        // `{` が続かなければ入力欄ではない（PowerShell の `$?` など）
        if bytes.get(cursor) != Some(&b'{') {
            i += 1;
            continue;
        }

        let keyword = &text[keyword_start..cursor];
        let Some(rule) = Rule::from_keyword(keyword) else {
            return Some(format!(
                "$? に未知の指定があります: {}（使えるのは int / num / name）",
                keyword
            ));
        };

        let inner_start = cursor + 1;
        let Some(end) = find_close(&bytes[inner_start..]) else {
            return Some("$?{ が } で閉じられていません".to_string());
        };

        let spec = &text[inner_start..inner_start + end];
        let (message, default_value) = split_spec(spec);

        if message.trim().is_empty() {
            return Some("$?{} に説明がありません（例: $?{長辺のピクセル数=1280}）".to_string());
        }

        // 入れ子は先に解決する順序が決められないので断る。
        // 説明の中のパスや日時のプレースホルダーは解決される
        if spec.contains("$?") {
            return Some("$?{} を入れ子にはできません".to_string());
        }

        // 既定値が決まりを満たしているかも、ここで見ておける。
        // プレースホルダーを含むものは実行してみないと分からないので飛ばす
        if !default_value.is_empty() && !default_value.contains('$') {
            if let Err(reason) = rule.check(rule.normalize(default_value)) {
                return Some(format!("$?{{}} の既定値が決まりに合いません: {}", reason));
            }
        }

        i = inner_start + end + 1;
    }

    None
}

/// 文字列に書かれた入力欄を、書かれている順に取り出す
///
/// `^$` で打ち消したものは含めない。同じものが複数あってもそのまま並ぶので、
/// 重複の除去は呼び出し側が担う。
pub fn prompts(text: &str) -> Vec<Prompt<'_>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'^' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }

        match parse_at(text, i) {
            Some((prompt, end)) => {
                found.push(prompt);
                i = end;
            }
            None => i += 1,
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;

    /// 書かれた入力欄の元の文字列（`RunContext` の見出しになるもの）
    fn sources(text: &str) -> Vec<&str> {
        prompts(text).into_iter().map(|p| p.source).collect()
    }

    /// 実際にダイアログを出して確かめる
    ///
    /// テンプレートの組み立てはコンパイラが検査できないので、実機で 1 度は
    /// 通しておきたい。ただし画面が要るうえに 1 秒ほどかかるため、既定では
    /// 走らせない。**`cargo test -- --ignored` で実行する。**
    ///
    /// **1 つのテストにまとめてあるのは、ダイアログを題名で探すため。**
    /// `cargo test` は既定でテストを並行して走らせるので、分けると互いの
    /// ダイアログを掴んでしまう。
    #[test]
    #[ignore = "画面が必要（cargo test -- --ignored で実行）"]
    fn ダイアログの操作をひととおり確かめる() {
        // --- 既定値をそのまま確定する ---
        let 操作 = std::thread::spawn(|| answer(&[None]));
        assert_eq!(
            ask(Rule::Any, "テスト用の入力欄", "既定値"),
            Some("既定値".to_string()),
            "既定値は選択された状態で入っているので、そのまま OK すれば返る"
        );
        操作.join().expect("操作のスレッドが終わる");

        // --- 決まりを満たさない入力は聞き直す ---
        let 操作 = std::thread::spawn(|| answer(&[Some("1280px"), Some("1280")]));
        assert_eq!(
            ask(Rule::Int, "長辺のピクセル数", ""),
            Some("1280".to_string()),
            "整数でない入力では打ち切らず、聞き直して 2 回目の値を返す"
        );
        操作.join().expect("操作のスレッドが終わる");

        // --- キャンセルすると答えが返らない ---
        let 操作 = std::thread::spawn(cancel);
        assert_eq!(ask(Rule::Any, "取り消す入力欄", "既定値"), None);
        操作.join().expect("操作のスレッドが終わる");
    }

    /// 入力ダイアログが出るたびに、値を入れて OK を押す
    ///
    /// `None` は「入っている値のまま OK」。
    fn answer(values: &[Option<&str>]) {
        for value in values {
            let Some(hwnd) = wait_for_dialog() else {
                return;
            };

            if let Some(value) = value {
                let text = to_wide(value);
                unsafe {
                    let edit = GetDlgItem(hwnd, ID_EDIT as i32);
                    SendMessageW(edit, WM_SETTEXT, 0, text.as_ptr() as LPARAM);
                }
            }

            unsafe { PostMessageW(hwnd, WM_COMMAND, IDOK as WPARAM, 0) };
            wait_until_closed(hwnd);
        }
    }

    /// 入力ダイアログをキャンセルする
    fn cancel() {
        if let Some(hwnd) = wait_for_dialog() {
            unsafe { PostMessageW(hwnd, WM_COMMAND, IDCANCEL as WPARAM, 0) };
            wait_until_closed(hwnd);
        }
    }

    /// 入力ダイアログが現れるまで待つ
    fn wait_for_dialog() -> Option<HWND> {
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let hwnd = unsafe { FindWindowW(null_mut(), to_wide("ExtRun").as_ptr()) };
            if !hwnd.is_null() {
                return Some(hwnd);
            }
        }
        None
    }

    /// 次のダイアログと取り違えないよう、閉じきるまで待つ
    fn wait_until_closed(hwnd: HWND) {
        while unsafe { IsWindow(hwnd) } != 0 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
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
        assert_eq!(
            sources("-w $?{幅=1280} -h $?{高さ}"),
            vec!["$?{幅=1280}", "$?{高さ}"]
        );
        assert_eq!(sources("$p"), Vec::<&str>::new());
    }

    /// 中に `$t{...}` を書いても、最初の `}` で切れてしまわない
    #[test]
    fn 中の日時を巻き込んで終端を数える() {
        assert_eq!(
            sources("$?{新しい名前=$a_$t{yyyyMMdd}}"),
            vec!["$?{新しい名前=$a_$t{yyyyMMdd}}"]
        );
        assert_eq!(
            split_spec("新しい名前=$a_$t{yyyyMMdd}"),
            ("新しい名前", "$a_$t{yyyyMMdd}")
        );
        assert_eq!(validate("$?{新しい名前=$a_$t{yyyyMMdd}}"), None);
        // 後ろに続く指定も取りこぼさない
        assert_eq!(
            sources("$?{名前=$t{yyyy}} $?{幅}"),
            vec!["$?{名前=$t{yyyy}}", "$?{幅}"]
        );
    }

    #[test]
    fn エスケープしたものは取り出さない() {
        assert_eq!(sources("^$?{これは入力欄ではない}"), Vec::<&str>::new());
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

    // -----------------------------------------------------------------
    // 入力値の決まり
    // -----------------------------------------------------------------

    #[test]
    fn 決まりの語を読める() {
        let (prompt, _) = parse_at("$?int{長辺=1280}", 0).expect("読める");
        assert_eq!(prompt.rule, Rule::Int);
        assert_eq!(prompt.message, "長辺");
        assert_eq!(prompt.default_value, "1280");
        assert_eq!(prompt.source, "$?int{長辺=1280}");

        assert_eq!(parse_at("$?{幅}", 0).expect("読める").0.rule, Rule::Any);
        assert_eq!(parse_at("$?num{率}", 0).expect("読める").0.rule, Rule::Num);
        assert_eq!(
            parse_at("$?name{名}", 0).expect("読める").0.rule,
            Rule::Name
        );
    }

    /// 決まりが違えば別の入力欄として扱う（答えを取り違えない）
    #[test]
    fn 決まりの違う入力欄は別ものになる() {
        let found = sources("$?{幅} $?int{幅}");
        assert_eq!(found, vec!["$?{幅}", "$?int{幅}"]);
    }

    #[test]
    fn 整数の検査() {
        assert!(Rule::Int.check("1280").is_ok());
        assert!(Rule::Int.check("-5").is_ok());
        // 単位付き・小数・全角数字・空はすべて弾く
        for value in ["1280px", "12.5", "１２８０", "", "abc"] {
            assert!(Rule::Int.check(value).is_err(), "{} が通ってしまう", value);
        }
    }

    #[test]
    fn 数値の検査() {
        for value in ["1.5", "-2", "0.25"] {
            assert!(Rule::Num.check(value).is_ok(), "{} が弾かれる", value);
        }
        // 無限大は数値として解釈できてしまうので明示的に弾く
        for value in ["inf", "NaN", "", "1.5x"] {
            assert!(Rule::Num.check(value).is_err(), "{} が通ってしまう", value);
        }
    }

    #[test]
    fn ファイル名の検査() {
        assert!(Rule::Name.check("報告書 2026").is_ok());
        assert!(Rule::Name.check("a.b.c").is_ok());
    }

    #[test]
    fn ファイル名に使えない文字を弾く() {
        for value in [
            "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b",
        ] {
            let reason = Rule::Name.check(value).expect_err(value);
            assert!(reason.contains("使えない文字"), "{} → {}", value, reason);
        }
    }

    #[test]
    fn 空の名前と末尾の記号を弾く() {
        assert!(Rule::Name.check("").is_err());
        assert!(Rule::Name.check("名前.").is_err());
        assert!(Rule::Name.check("名前 ").is_err());
    }

    /// CON.txt のように拡張子を付けても予約名は使えない
    #[test]
    fn 予約された装置名を弾く() {
        for value in ["CON", "con", "NUL", "COM1", "CON.txt", "lpt9.log"] {
            let reason = Rule::Name.check(value).expect_err(value);
            assert!(
                reason.contains("予約している名前"),
                "{} → {}",
                value,
                reason
            );
        }
        // 予約名で始まるだけなら使える
        assert!(Rule::Name.check("CONTENTS").is_ok());
    }

    /// パス全体の上限は判定できないが、名前 1 つぶんの上限は決まっている
    #[test]
    fn 長すぎる名前を弾く() {
        assert!(Rule::Name.check(&"あ".repeat(MAX_NAME_LENGTH)).is_ok());
        let reason = Rule::Name
            .check(&"あ".repeat(MAX_NAME_LENGTH + 1))
            .unwrap_err();
        assert!(reason.contains("長すぎます"), "{}", reason);
    }

    /// 数値と名前では前後の空白が意味を持たない
    #[test]
    fn 前後の空白を落とす() {
        assert_eq!(Rule::Int.normalize(" 1280 "), "1280");
        assert_eq!(Rule::Name.normalize(" 名前 "), "名前");
        // 制限なしのときは打たれたとおりに渡す
        assert_eq!(Rule::Any.normalize(" そのまま "), " そのまま ");
    }

    #[test]
    fn 未知の決まりを検出する() {
        let message = validate("$?Int{幅}").expect("エラーになる");
        assert!(message.contains("未知の指定"), "{}", message);
        assert!(message.contains("int / num / name"), "{}", message);
    }

    /// 設定を書いた時点で気づけるので、既定値も検証しておく
    #[test]
    fn 決まりに合わない既定値を検出する() {
        let message = validate("$?int{品質=たかい}").expect("エラーになる");
        assert!(
            message.contains("既定値が決まりに合いません"),
            "{}",
            message
        );
        assert!(message.contains("整数"), "{}", message);

        let message = validate("$?name{名前=a/b}").expect("エラーになる");
        assert!(
            message.contains("既定値が決まりに合いません"),
            "{}",
            message
        );
    }

    /// 既定値にプレースホルダーが入っていると、実行するまで値が決まらない
    #[test]
    fn プレースホルダーを含む既定値は検証しない() {
        assert_eq!(validate("$?int{幅=$e}"), None);
        assert_eq!(validate("$?name{名前=$a_$t{yyyyMMdd}}"), None);
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
