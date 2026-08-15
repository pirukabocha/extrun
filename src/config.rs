/*!
設定ファイル（extrun-config.txt）の読み込みとパース

書式の仕様は extrun-config-format.md を参照。
*/

use std::fs;
use std::path::Path;

/// 設定ファイル名
pub const CONFIG_FILE_NAME: &str = "extrun-config.txt";

/// エスケープ（`^`）の対象になる特殊文字
///
/// `placeholder.rs` も同じ定義を使う。引数のエスケープはパース時ではなく
/// 実行時に解決されるので、2 か所で食い違うと `^X` が片方だけ素通りする。
pub(crate) const SPECIALS: &[u8] = b"^@$|:>+-#[]&";

/// 別名の入れ子の深さの上限
const MAX_ALIAS_DEPTH: usize = 32;

/// グローバル設定のセクション名（`[extrun]`）
///
/// 拡張子は `.` で始まる必要がある（`file` / `folder` だけが例外）ので、
/// この名前は元々エラーになる書き方だった。予約しても既存の設定を壊さない。
const SETTINGS_SECTION: &str = "extrun";

/// 診断メッセージの種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// メニューを表示できない致命的な問題
    Error,
    /// 表示はできるが書き間違いの可能性がある問題
    Warning,
}

/// 行番号付きの診断メッセージ
#[derive(Debug, Clone)]
pub struct Diag {
    pub line: u32,
    pub severity: Severity,
    pub message: String,
}

impl Diag {
    fn error(line: u32, message: String) -> Self {
        Diag {
            line,
            severity: Severity::Error,
            message,
        }
    }

    pub fn warning(line: u32, message: String) -> Self {
        Diag {
            line,
            severity: Severity::Warning,
            message,
        }
    }
}

/// メニュー項目
#[derive(Debug, Clone, Default)]
pub struct MenuItem {
    /// メニューに表示される名前
    ///
    /// アクセスキーの `&` は取り除き済み（`^&` は素の `&` に解決済み）。
    /// Win32 に渡すラベルは `menu.rs` がここから組み立て直す。
    pub name: String,
    /// アクセスキーの位置（`name` のバイト位置。そこには必ず ASCII の英数字がある）
    pub accesskey: Option<usize>,
    /// 対象の拡張子（継承・引き算・置換を解決済み。空ならすべて対象）
    pub extensions: Vec<String>,
    /// 起動する実行ファイル
    pub path: String,
    /// 引数（`^` を残したまま保持し、実行時に置換する）
    pub args: Vec<String>,
    /// 作業フォルダ（空ならパスの親フォルダ）
    pub working_dir: String,
    /// 実行前に確認する（`:confirm`）
    ///
    /// `None` は確認なし。`Some` の中身は添えるメッセージで、`:confirm` に値を
    /// 書かなかった場合は空文字列になる。`^` は残したまま保持し、実行時に
    /// プレースホルダーと一緒に解決する（`working_dir` と同じ扱い）。
    pub confirm: Option<String>,
    /// 複数選択時にすべてまとめて 1 プロセスへ渡すか（`+`）
    pub all_mode: bool,
    /// サブメニューの子項目
    pub submenu: Vec<MenuItem>,
    /// セパレーター（`---`）かどうか
    pub separator: bool,
    /// 定義されている行番号
    pub line: u32,
}

impl MenuItem {
    /// セパレーターかどうか
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// サブメニューを持つかどうか
    pub fn has_submenu(&self) -> bool {
        !self.submenu.is_empty()
    }

    /// アクセスキーの文字（大文字に正規化。Win32 は大小を区別しない）
    pub fn accesskey_char(&self) -> Option<char> {
        let pos = self.accesskey?;
        self.name[pos..]
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
    }
}

/// メニューを表示する位置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuPosition {
    /// マウスカーソルの位置（既定）
    #[default]
    Cursor,
    /// 前面ウィンドウの中央
    Window,
    /// 画面（タスクバーを除く作業領域）の中央
    Screen,
    /// 画面座標を直接指定（物理ピクセル）
    Point { x: i32, y: i32 },
}

/// アプリ全体のふるまい（`[extrun]` セクション）
///
/// 既定値は 1.0.0 までのふるまいと同じ（カーソル位置・選択なし）。
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// メニューを表示する位置
    pub menu_position: MenuPosition,
    /// 先頭項目を選択した状態でメニューを開くか
    pub select_first: bool,
}

/// 設定ファイルの内容
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub apps: Vec<MenuItem>,
    pub settings: Settings,
}

/// パース結果（診断メッセージ付き）
pub struct Parsed {
    pub config: Config,
    pub diags: Vec<Diag>,
}

impl Parsed {
    /// エラーの診断メッセージだけを取り出す
    pub fn errors(&self) -> impl Iterator<Item = &Diag> {
        self.diags.iter().filter(|d| d.severity == Severity::Error)
    }

    /// エラーがあるかどうか
    pub fn has_error(&self) -> bool {
        self.errors().next().is_some()
    }
}

impl Config {
    /// 設定ファイルを読み込んでパースする
    ///
    /// 戻り値の `Err` はファイルが読めない・文字コードが不正といった
    /// 行番号を持たない問題のみ。書式の問題は `Parsed::diags` に入る。
    pub fn load(path: &Path) -> Result<Parsed, String> {
        let bytes = fs::read(path)
            .map_err(|e| format!("設定ファイルを読み込めません:\n{}\n\n{}", path.display(), e))?;

        let text = decode_utf8(&bytes).ok_or_else(|| {
            format!(
                "設定ファイルが UTF-8 ではありません:\n{}\n\nUTF-8 で保存し直してください。",
                path.display()
            )
        })?;

        Ok(parse(&text))
    }
}

/// BOM を読み飛ばして UTF-8 として解釈する
fn decode_utf8(bytes: &[u8]) -> Option<String> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(body.to_vec()).ok()
}

// =====================================================================
// 文字列ユーティリティ
// =====================================================================

/// エスケープの対象になる文字か
fn is_special(b: u8) -> bool {
    SPECIALS.contains(&b)
}

/// UTF-8 の 1 文字のバイト数
fn char_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// `^X`（X は特殊文字）ならその長さを返す
fn escape_len(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes[i] == b'^' && i + 1 < bytes.len() && is_special(bytes[i + 1]) {
        Some(2)
    } else {
        None
    }
}

/// エスケープされていない区切り文字で分割する
fn split_unescaped(text: &str, sep: u8) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;

    while i < bytes.len() {
        if let Some(len) = escape_len(bytes, i) {
            i += len;
            continue;
        }
        if bytes[i] == sep {
            parts.push(&text[start..i]);
            start = i + 1;
        }
        i += char_len(bytes[i]);
    }

    parts.push(&text[start..]);
    parts
}

/// `^` によるエスケープを解決する（名前・パス用）
fn unescape(text: &str) -> String {
    if !text.contains('^') {
        return text.to_string();
    }

    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut chunk = 0;
    let mut i = 0;

    while i < bytes.len() {
        if let Some(len) = escape_len(bytes, i) {
            out.push_str(&text[chunk..i]);
            out.push(bytes[i + 1] as char);
            i += len;
            chunk = i;
            continue;
        }
        i += char_len(bytes[i]);
    }

    out.push_str(&text[chunk..]);
    out
}

/// アクセスキーになれる文字か（半角英数字）
fn is_accesskey_char(b: Option<&u8>) -> bool {
    matches!(b, Some(b) if b.is_ascii_alphanumeric())
}

/// エスケープを解決しつつ、アクセスキーの `&` を読み取る（名前用）
///
/// `unescape()` と 1 パスで兼ねる必要がある。先にエスケープだけを解決すると
/// `^&` が `&` になったあとアクセスキーの目印として拾われてしまう。
/// 引数の `^$` とプレースホルダーの関係と同じ理由。
///
/// 戻り値の 2 つ目は、アクセスキーの文字が表示名の何バイト目から始まるか。
/// 有効な `&` が複数あれば最初のものだけを使い、残りはただの `&` として残す
/// （`warn_accesskey` が別に警告する）。
fn unescape_name(text: &str) -> (String, Option<usize>) {
    if !text.contains('^') && !text.contains('&') {
        return (text.to_string(), None);
    }

    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut accesskey = None;
    let mut chunk = 0;
    let mut i = 0;

    while i < bytes.len() {
        if let Some(len) = escape_len(bytes, i) {
            out.push_str(&text[chunk..i]);
            out.push(bytes[i + 1] as char);
            i += len;
            chunk = i;
            continue;
        }

        // `&` の直後が半角英数字ならアクセスキー。目印の `&` は表示から落とす
        if bytes[i] == b'&' && accesskey.is_none() && is_accesskey_char(bytes.get(i + 1)) {
            out.push_str(&text[chunk..i]);
            accesskey = Some(out.len());
            i += 1;
            chunk = i;
            continue;
        }

        i += char_len(bytes[i]);
    }

    out.push_str(&text[chunk..]);
    (out, accesskey)
}

/// アクセスキーの `&` の書き間違いを警告する（種類ごとに 1 フィールド 1 件）
fn warn_accesskey(text: &str, line: u32, diags: &mut Vec<Diag>) {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut warned_shape = false;
    let mut i = 0;

    while i < bytes.len() {
        if let Some(len) = escape_len(bytes, i) {
            i += len;
            continue;
        }

        if bytes[i] == b'&' {
            if is_accesskey_char(bytes.get(i + 1)) {
                count += 1;
                if count == 2 {
                    diags.push(Diag::warning(
                        line,
                        "アクセスキーの & が複数あります（最初のものだけが有効です）".to_string(),
                    ));
                }
            } else if !warned_shape {
                warned_shape = true;
                diags.push(Diag::warning(
                    line,
                    "アクセスキーの & の後ろは半角英数字にしてください（& そのものを書くには ^&）"
                        .to_string(),
                ));
            }
            i += 1;
            continue;
        }

        i += char_len(bytes[i]);
    }
}

/// 何もエスケープしていない `^` を警告する（1 フィールドにつき 1 件）
fn warn_stray_caret(text: &str, line: u32, diags: &mut Vec<Diag>) {
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'^' {
            match escape_len(bytes, i) {
                Some(len) => i += len,
                None => {
                    diags.push(Diag::warning(
                        line,
                        "エスケープになっていない ^ があります".to_string(),
                    ));
                    return;
                }
            }
            continue;
        }
        i += char_len(bytes[i]);
    }
}

// =====================================================================
// 別名
// =====================================================================

/// 別名の定義と解決結果
struct Aliases {
    /// 名前・未解決の値・定義行
    raw: Vec<(String, String, u32)>,
    /// 解決済みの値（失敗した別名は空文字で記録し、報告を 1 回にとどめる）
    resolved: Vec<(String, String)>,
}

impl Aliases {
    fn new() -> Self {
        Aliases {
            raw: Vec::new(),
            resolved: Vec::new(),
        }
    }

    fn define(&mut self, name: &str, value: &str, line: u32, diags: &mut Vec<Diag>) {
        if self.raw.iter().any(|(n, _, _)| n == name) {
            diags.push(Diag::error(
                line,
                format!("別名 @{} が重複して定義されています", name),
            ));
            return;
        }
        self.raw.push((name.to_string(), value.to_string(), line));
    }

    /// 別名の値を解決する（未定義・循環は診断を出して `None`）
    fn value(
        &mut self,
        name: &str,
        stack: &mut Vec<String>,
        use_line: u32,
        diags: &mut Vec<Diag>,
    ) -> Option<String> {
        if let Some((_, value)) = self.resolved.iter().find(|(n, _)| n == name) {
            return Some(value.clone());
        }

        let Some((_, raw, def_line)) = self.raw.iter().find(|(n, _, _)| n == name).cloned() else {
            diags.push(Diag::error(
                use_line,
                format!("未定義の別名 @{} を使用しています", name),
            ));
            self.resolved.push((name.to_string(), String::new()));
            return None;
        };

        if stack.iter().any(|n| n == name) {
            diags.push(Diag::error(
                def_line,
                format!("別名 @{} が循環参照しています", name),
            ));
            self.resolved.push((name.to_string(), String::new()));
            return None;
        }

        if stack.len() >= MAX_ALIAS_DEPTH {
            diags.push(Diag::error(
                def_line,
                format!("別名 @{} の入れ子が深すぎます", name),
            ));
            self.resolved.push((name.to_string(), String::new()));
            return None;
        }

        stack.push(name.to_string());
        let value = self.expand_inner(&raw, stack, def_line, diags);
        stack.pop();

        self.resolved.push((name.to_string(), value.clone()));
        Some(value)
    }

    /// 定義されているすべての別名を解決する
    ///
    /// 使われている別名は展開時に解決済みなので、ここで新たに診断が出るのは
    /// どこからも使われていない別名だけ。エラーは定義行に付く。
    fn resolve_all(&mut self, diags: &mut Vec<Diag>) {
        let names: Vec<(String, u32)> = self
            .raw
            .iter()
            .map(|(name, _, line)| (name.clone(), *line))
            .collect();

        for (name, line) in names {
            let mut stack = Vec::new();
            self.value(&name, &mut stack, line, diags);
        }
    }

    /// 文字列中の `@名前` を展開する
    fn expand(&mut self, text: &str, line: u32, diags: &mut Vec<Diag>) -> String {
        if !text.contains('@') {
            return text.to_string();
        }
        let mut stack = Vec::new();
        self.expand_inner(text, &mut stack, line, diags)
    }

    fn expand_inner(
        &mut self,
        text: &str,
        stack: &mut Vec<String>,
        line: u32,
        diags: &mut Vec<Diag>,
    ) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut chunk = 0;
        let mut i = 0;

        while i < bytes.len() {
            // `^@` は別名として解決しない（`^` は後段のために残す）
            if let Some(len) = escape_len(bytes, i) {
                i += len;
                continue;
            }

            if bytes[i] != b'@' {
                i += char_len(bytes[i]);
                continue;
            }

            let name_end = alias_name_end(bytes, i + 1);
            if name_end == i + 1 {
                i += 1;
                continue;
            }

            out.push_str(&text[chunk..i]);
            let name = &text[i + 1..name_end];
            if let Some(value) = self.value(name, stack, line, diags) {
                out.push_str(&value);
            }
            i = name_end;
            chunk = i;
        }

        out.push_str(&text[chunk..]);
        out
    }
}

/// `@名前` の名前が終わる位置を返す（空白 / `\` / `|` / `]` / 行末で終わる）
fn alias_name_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\\' | b'|' | b']' | b'[' | b'^' | b'@' => break,
            _ => i += char_len(bytes[i]),
        }
    }
    i
}

// =====================================================================
// パース
// =====================================================================

/// 論理行（継続行を連結したあとの 1 項目）
struct ItemLine {
    line: u32,
    text: String,
    working_dir: Option<(u32, String)>,
    /// `:confirm`（値は省略できるので空文字列になりうる）
    confirm: Option<(u32, String)>,
}

/// 行の種類
enum Stmt {
    Alias {
        line: u32,
        name: String,
        value: String,
    },
    Section {
        line: u32,
        spec: String,
    },
    Item(ItemLine),
}

/// 設定ファイルの内容をパースする
pub fn parse(text: &str) -> Parsed {
    let mut diags = Vec::new();
    let stmts = split_statements(text, &mut diags);

    // 1 パス目: 別名を集める
    let mut aliases = Aliases::new();
    for stmt in &stmts {
        if let Stmt::Alias { line, name, value } = stmt {
            warn_stray_caret(value, *line, &mut diags);
            aliases.define(name, value, *line, &mut diags);
        }
    }

    // 2 パス目: セクションと項目を組み立てる
    let config = build_menu(&stmts, &mut aliases, &mut diags);

    // どこからも使われていない別名も解決して、循環や未定義を取りこぼさない
    aliases.resolve_all(&mut diags);

    diags.sort_by_key(|d| d.line);
    Parsed { config, diags }
}

/// 物理行を論理行に分解する（継続行の連結・コメントの除去）
fn split_statements(text: &str, diags: &mut Vec<Diag>) -> Vec<Stmt> {
    let mut stmts: Vec<Stmt> = Vec::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line = index as u32 + 1;
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // 継続行
        if let Some(rest) = trimmed.strip_prefix('|') {
            match stmts.last_mut() {
                Some(Stmt::Item(item)) => {
                    item.text.push_str(" | ");
                    item.text.push_str(rest.trim());
                }
                _ => diags.push(Diag::error(
                    line,
                    "継続行（|）の前に項目がありません".to_string(),
                )),
            }
            continue;
        }

        // 名前付きフィールド
        if let Some(rest) = trimmed.strip_prefix(':') {
            let (keyword, value) = split_keyword(rest);
            if !matches!(keyword, "dir" | "confirm") {
                diags.push(Diag::error(
                    line,
                    format!(": の後に未知のキーワードがあります: {}", keyword),
                ));
                continue;
            }

            match stmts.last_mut() {
                Some(Stmt::Item(item)) => {
                    let field = match keyword {
                        "dir" => &mut item.working_dir,
                        _ => &mut item.confirm,
                    };
                    *field = Some((line, value.to_string()));
                }
                _ => diags.push(Diag::error(
                    line,
                    format!(":{} の前に項目がありません", keyword),
                )),
            }
            continue;
        }

        // 別名の定義
        if let Some((name, value)) = as_alias_def(trimmed) {
            stmts.push(Stmt::Alias {
                line,
                name: name.to_string(),
                value: value.to_string(),
            });
            continue;
        }

        // セクション見出し
        if let Some(spec) = as_section(trimmed) {
            stmts.push(Stmt::Section {
                line,
                spec: spec.to_string(),
            });
            continue;
        }

        stmts.push(Stmt::Item(ItemLine {
            line,
            text: trimmed.to_string(),
            working_dir: None,
            confirm: None,
        }));
    }

    stmts
}

/// `:キーワード 値` を分解する
fn split_keyword(rest: &str) -> (&str, &str) {
    let rest = rest.trim_start();
    match rest.find(char::is_whitespace) {
        Some(pos) => (&rest[..pos], rest[pos..].trim()),
        None => (rest, ""),
    }
}

/// `@名前 = 値` なら名前と値を返す
fn as_alias_def(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('@')?;
    let eq = rest.find('=')?;
    let name = rest[..eq].trim_end();

    if name.is_empty() || name.contains(|c: char| c.is_whitespace() || c == '|' || c == '[') {
        return None;
    }

    Some((name, rest[eq + 1..].trim()))
}

/// `[...]` だけの行ならその中身を返す
fn as_section(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    Some(inner)
}

/// 表示位置の値を解釈する
///
/// 設定ファイルの `menu-position` とコマンドラインの `--at` で共有する。
/// 片方だけに書き方が増えるとずれるので、解釈はここ 1 か所に集める。
///
/// `X,Y` の座標は物理ピクセル。マルチモニタでは主モニタより左や上にある画面の
/// 座標が負になるので、符号付きで受ける。
pub fn parse_menu_position(value: &str) -> Option<MenuPosition> {
    let value = value.trim();

    for (keyword, position) in [
        ("cursor", MenuPosition::Cursor),
        ("window", MenuPosition::Window),
        ("screen", MenuPosition::Screen),
    ] {
        if value.eq_ignore_ascii_case(keyword) {
            return Some(position);
        }
    }

    let (x, y) = value.split_once(',')?;
    Some(MenuPosition::Point {
        x: x.trim().parse().ok()?,
        y: y.trim().parse().ok()?,
    })
}

/// `yes` / `no` を解釈する
pub fn parse_yes_no(value: &str) -> Option<bool> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("yes") {
        Some(true)
    } else if value.eq_ignore_ascii_case("no") {
        Some(false)
    } else {
        None
    }
}

/// `[extrun]` セクションの `名前 = 値` を読む
fn parse_setting(
    item: &ItemLine,
    settings: &mut Settings,
    seen: &mut Vec<String>,
    diags: &mut Vec<Diag>,
) {
    let line = item.line;

    for (keyword, present) in [
        ("dir", item.working_dir.is_some()),
        ("confirm", item.confirm.is_some()),
    ] {
        if present {
            diags.push(Diag::error(
                line,
                format!("[{}] の中では :{} を使えません", SETTINGS_SECTION, keyword),
            ));
        }
    }

    let Some((key, value)) = item.text.split_once('=') else {
        diags.push(Diag::error(
            line,
            format!(
                "[{}] の設定は 名前 = 値 の形で書きます: {}",
                SETTINGS_SECTION, item.text
            ),
        ));
        return;
    };

    let key = key.trim().to_ascii_lowercase();
    let value = value.trim();

    if seen.contains(&key) {
        diags.push(Diag::error(line, format!("設定が重複しています: {}", key)));
        return;
    }

    match key.as_str() {
        "menu-position" => match parse_menu_position(value) {
            Some(position) => settings.menu_position = position,
            None => diags.push(Diag::error(
                line,
                format!(
                    "menu-position の値が不正です（cursor / window / screen / X,Y）: {}",
                    value
                ),
            )),
        },
        "select-first" => match parse_yes_no(value) {
            Some(flag) => settings.select_first = flag,
            None => diags.push(Diag::error(
                line,
                format!("select-first の値が不正です（yes / no）: {}", value),
            )),
        },
        _ => {
            diags.push(Diag::error(
                line,
                format!("[{}] に未知の設定があります: {}", SETTINGS_SECTION, key),
            ));
            return;
        }
    }

    // 値が不正でも「書かれてはいた」ので重複判定には数える
    seen.push(key);
}

/// セクションと項目からメニューを組み立てる
fn build_menu(stmts: &[Stmt], aliases: &mut Aliases, diags: &mut Vec<Diag>) -> Config {
    let mut root: Vec<MenuItem> = Vec::new();
    // 開いているサブメニューの親。stack[i] が階層 i の項目
    let mut stack: Vec<MenuItem> = Vec::new();
    let mut section: Option<Vec<String>> = None;
    let mut reported_missing_section = false;
    let mut settings = Settings::default();
    let mut seen_settings: Vec<String> = Vec::new();
    // `[extrun]` の中にいるか。拡張子セクションが来ると抜ける
    let mut in_settings = false;

    for stmt in stmts {
        match stmt {
            Stmt::Alias { .. } => {}

            Stmt::Section { line, spec } => {
                close_submenus(&mut stack, 0, &mut root);
                in_settings = spec.trim().eq_ignore_ascii_case(SETTINGS_SECTION);
                if in_settings {
                    continue;
                }
                let expanded = aliases.expand(spec, *line, diags);
                section = Some(parse_extensions(&expanded, &[], false, *line, diags));
            }

            Stmt::Item(item) => {
                if in_settings {
                    parse_setting(item, &mut settings, &mut seen_settings, diags);
                    continue;
                }

                let Some(section_ext) = section.as_ref() else {
                    if !reported_missing_section {
                        reported_missing_section = true;
                        diags.push(Diag::error(
                            item.line,
                            "項目より前にセクション見出し（[...]）がありません".to_string(),
                        ));
                    }
                    continue;
                };

                let (mut depth, all_mode, rest) = parse_markers(&item.text);

                if depth > stack.len() {
                    diags.push(Diag::error(
                        item.line,
                        format!(
                            "サブメニューの階層が飛んでいます（> は {} 個まで）",
                            stack.len()
                        ),
                    ));
                    depth = stack.len();
                }

                close_submenus(&mut stack, depth, &mut root);

                let inherited = match stack.last() {
                    Some(parent) => parent.extensions.clone(),
                    None => section_ext.clone(),
                };

                let menu_item = build_item(rest, item, all_mode, &inherited, aliases, diags);
                stack.push(menu_item);
            }
        }
    }

    close_submenus(&mut stack, 0, &mut root);
    Config {
        apps: root,
        settings,
    }
}

/// 階層 `depth` より深いサブメニューを閉じる
fn close_submenus(stack: &mut Vec<MenuItem>, depth: usize, root: &mut Vec<MenuItem>) {
    while stack.len() > depth {
        let item = stack.pop().expect("stack は空ではない");
        match stack.last_mut() {
            Some(parent) => parent.submenu.push(item),
            None => root.push(item),
        }
    }
}

/// 1 項目を組み立てる
fn build_item(
    text: &str,
    source: &ItemLine,
    all_mode: bool,
    inherited: &[String],
    aliases: &mut Aliases,
    diags: &mut Vec<Diag>,
) -> MenuItem {
    let line = source.line;
    let fields = split_unescaped(text, b'|');

    if fields.len() > 3 {
        diags.push(Diag::error(
            line,
            "項目のフィールドが多すぎます（名前 | パス | 引数）".to_string(),
        ));
    }

    // 名前と拡張子
    let (name_part, ext_spec) = split_ext_spec(fields[0].trim());
    let extensions = match ext_spec {
        Some(spec) => {
            let expanded = aliases.expand(spec, line, diags);
            parse_extensions(&expanded, inherited, true, line, diags)
        }
        None => inherited.to_vec(),
    };

    warn_stray_caret(name_part, line, diags);
    let expanded = aliases.expand(name_part, line, diags);
    warn_accesskey(&expanded, line, diags);
    let (name, accesskey) = unescape_name(&expanded);
    let separator = name == "---";

    // パス
    let path = match fields.get(1) {
        Some(field) => {
            let field = field.trim();
            warn_stray_caret(field, line, diags);
            unescape(&aliases.expand(field, line, diags))
        }
        None => String::new(),
    };

    // 引数（`^` は残したまま。実行時のプレースホルダー置換で解決する）
    let args = match fields.get(2) {
        Some(field) => {
            let field = field.trim();
            warn_stray_caret(field, line, diags);
            split_args(&aliases.expand(field, line, diags))
        }
        None => vec!["$p".to_string()],
    };

    // 作業フォルダ（同じく `^` を残す）
    let working_dir = match &source.working_dir {
        Some((dir_line, value)) => {
            warn_stray_caret(value, *dir_line, diags);
            aliases.expand(value, *dir_line, diags)
        }
        None => String::new(),
    };

    // 確認メッセージ（同じく `^` を残す。値なしの `:confirm` は空文字列）
    let confirm = source.confirm.as_ref().map(|(confirm_line, value)| {
        warn_stray_caret(value, *confirm_line, diags);
        aliases.expand(value, *confirm_line, diags)
    });

    // 日時と入力欄の書式はここで検証する。書き間違えると誤った文字列が黙って
    // ファイル名に入るので、警告ではなくエラーにしてメニューを止める
    for text in args
        .iter()
        .chain(std::iter::once(&working_dir))
        .chain(confirm.iter())
    {
        let problem = crate::datetime::validate(text).or_else(|| crate::prompt::validate(text));
        if let Some(message) = problem {
            diags.push(Diag::error(line, message));
        }
    }

    MenuItem {
        name,
        // セパレーターは選べないのでアクセスキーを持たない
        accesskey: if separator { None } else { accesskey },
        extensions,
        path,
        args,
        working_dir,
        confirm,
        all_mode,
        submenu: Vec::new(),
        separator,
        line,
    }
}

/// 行頭マーカー（`>` と `+`）を読み取る
///
/// マーカーは後ろに空白が続くときだけ有効。`+新規作成` はそのまま名前になる。
fn parse_markers(text: &str) -> (usize, bool, &str) {
    let mut depth = 0;
    let mut all_mode = false;
    let mut rest = text;

    loop {
        let bytes = rest.as_bytes();
        if bytes.is_empty() {
            break;
        }

        if bytes[0] == b'>' && depth == 0 {
            let count = bytes.iter().take_while(|&&b| b == b'>').count();
            if count < bytes.len() && is_blank(bytes[count]) {
                depth = count;
                rest = rest[count..].trim_start();
                continue;
            }
        }

        if bytes[0] == b'+' && !all_mode && bytes.len() > 1 && is_blank(bytes[1]) {
            all_mode = true;
            rest = rest[1..].trim_start();
            continue;
        }

        break;
    }

    (depth, all_mode, rest)
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// 名前の末尾にある `[...]` を切り離す
fn split_ext_spec(text: &str) -> (&str, Option<&str>) {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[bytes.len() - 1] != b']' {
        return (text, None);
    }

    let mut last_open = None;
    let mut last_escaped = false;
    let mut i = 0;

    while i < bytes.len() {
        if let Some(len) = escape_len(bytes, i) {
            last_escaped = i + len == bytes.len();
            i += len;
            continue;
        }
        if bytes[i] == b'[' {
            last_open = Some(i);
        }
        i += char_len(bytes[i]);
    }

    // 末尾の `]` がエスケープされていた場合は拡張子指定ではない
    if last_escaped {
        return (text, None);
    }

    match last_open {
        Some(pos) => (
            text[..pos].trim_end(),
            Some(&text[pos + 1..bytes.len() - 1]),
        ),
        None => (text, None),
    }
}

/// 拡張子の指定を解決する
///
/// - すべて `-` 付き → 継承したものから引く
/// - `-` なしが 1 つでもある → 継承を無視して置き換える
/// - 混在 → エラー
fn parse_extensions(
    spec: &str,
    inherited: &[String],
    allow_subtract: bool,
    line: u32,
    diags: &mut Vec<Diag>,
) -> Vec<String> {
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();

    for token in spec.split_whitespace() {
        let (subtract, body) = match token.strip_prefix('-') {
            Some(body) => (true, body),
            None => (false, token),
        };

        let value = unescape(body).to_lowercase();
        if value.is_empty() {
            continue;
        }

        if value != "file" && value != "folder" && !value.starts_with('.') {
            diags.push(Diag::error(
                line,
                format!("拡張子の指定に先頭の . がありません: {}", value),
            ));
            continue;
        }

        if subtract {
            if !allow_subtract {
                diags.push(Diag::error(
                    line,
                    format!("セクション見出しでは引き算（-）は使えません: -{}", value),
                ));
                continue;
            }
            removed.push(value);
        } else {
            added.push(value);
        }
    }

    if !added.is_empty() && !removed.is_empty() {
        diags.push(Diag::error(
            line,
            "拡張子の指定で - 付きと - なしが混在しています".to_string(),
        ));
        return inherited.to_vec();
    }

    if !added.is_empty() {
        added.dedup();
        return added;
    }

    if !removed.is_empty() {
        return inherited
            .iter()
            .filter(|ext| !removed.iter().any(|r| r == *ext))
            .cloned()
            .collect();
    }

    inherited.to_vec()
}

/// 引数を空白区切りで分解する（引用符で空白を含められる）
#[cfg(test)]
mod split_args_tests {
    use super::*;

    /// `$t{...}` と `$?{...}` の中の空白では区切らない
    #[test]
    fn プレースホルダーの中括弧の中は区切らない() {
        assert_eq!(
            split_args("$t{yyyy-MM-dd HH:mm} $p"),
            vec!["$t{yyyy-MM-dd HH:mm}", "$p"]
        );
        assert_eq!(
            split_args("$?{$n の新しい名前}"),
            vec!["$?{$n の新しい名前}"]
        );
        // 入れ子も数えられる
        assert_eq!(
            split_args("$?{新しい名前=$a $t{yyyy MM}} -f"),
            vec!["$?{新しい名前=$a $t{yyyy MM}}", "-f"]
        );
    }

    /// 素の `{` まで数えると、PowerShell のスクリプトブロックが繋がってしまう
    #[test]
    fn 素の中括弧は数えない() {
        assert_eq!(
            split_args("-Command { $_ } -Other"),
            vec!["-Command", "{", "$_", "}", "-Other"]
        );
    }

    #[test]
    fn 引用符はこれまでどおり効く() {
        assert_eq!(
            split_args("-Command \"Get-Item 'a b'\" $p"),
            vec!["-Command", "Get-Item 'a b'", "$p"]
        );
    }
}

#[cfg(test)]
mod confirm_tests {
    use super::*;

    fn item_of(text: &str) -> MenuItem {
        let parsed = parse(text);
        let errors: Vec<&str> = parsed.errors().map(|d| d.message.as_str()).collect();
        assert!(errors.is_empty(), "予期しないエラー: {:?}", errors);
        parsed.config.apps.into_iter().next().expect("項目がある")
    }

    #[test]
    fn 確認なしが既定() {
        let item = item_of("[.txt]\nA | C:\\Windows\\notepad.exe");
        assert_eq!(item.confirm, None);
    }

    /// 値を書かない `:confirm` も有効。既定のメッセージが使われる
    #[test]
    fn 値のない確認は空文字列になる() {
        let item = item_of("[.txt]\nA | C:\\Windows\\notepad.exe\n :confirm");
        assert_eq!(item.confirm.as_deref(), Some(""));
    }

    #[test]
    fn 確認のメッセージを読める() {
        let item = item_of("[.txt]\nA | C:\\Windows\\notepad.exe\n :confirm 元に戻せません");
        assert_eq!(item.confirm.as_deref(), Some("元に戻せません"));
    }

    /// `:dir` と同じく、プレースホルダーは実行時に解決するので `^` を残す
    #[test]
    fn 確認のメッセージはエスケープを残す() {
        let item = item_of("[.txt]\nA | C:\\Windows\\notepad.exe\n :confirm $n を ^$p にします");
        assert_eq!(item.confirm.as_deref(), Some("$n を ^$p にします"));
    }

    #[test]
    fn 確認のメッセージでも別名が展開される() {
        let item = item_of(
            "@out = C:\\out\n[.txt]\nA | C:\\Windows\\notepad.exe\n :confirm @out に書き出します",
        );
        assert_eq!(item.confirm.as_deref(), Some("C:\\out に書き出します"));
    }

    #[test]
    fn 確認のメッセージの日時も検証される() {
        let parsed = parse("[.txt]\nA | C:\\Windows\\notepad.exe\n :confirm $t{yyyy/MM/dz} に実行");
        assert!(parsed.has_error());
    }

    #[test]
    fn 項目の前の確認はエラー() {
        let messages: Vec<String> = parse("[.txt]\n :confirm ためし")
            .errors()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains(":confirm の前に項目が")),
            "{:?}",
            messages
        );
    }

    #[test]
    fn 設定セクションの中では使えない() {
        let messages: Vec<String> = parse("[extrun]\nselect-first = yes\n :confirm ためし")
            .errors()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains(":confirm を使えません")),
            "{:?}",
            messages
        );
    }

    #[test]
    fn 未知のキーワードはこれまでどおりエラー() {
        let messages: Vec<String> = parse("[.txt]\nA | C:\\a.exe\n :verify はい")
            .errors()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains("未知のキーワード")),
            "{:?}",
            messages
        );
    }
}

#[cfg(test)]
mod datetime_tests {
    use super::*;

    fn errors(text: &str) -> Vec<String> {
        parse(text)
            .errors()
            .map(|diag| diag.message.clone())
            .collect()
    }

    #[test]
    fn 日時の書式は引数の中でも検証される() {
        let messages = errors("[.txt]\nA | C:\\Windows\\notepad.exe | $-p_$t{yyyyMMdd_backup}.zip");
        assert_eq!(messages.len(), 1, "{:?}", messages);
        assert!(messages[0].contains("書式ではない英字"), "{:?}", messages);
    }

    #[test]
    fn 日時の書式は作業フォルダでも検証される() {
        let messages =
            errors("[.txt]\nA | C:\\Windows\\notepad.exe\n :dir C:\\out\\$t{yyyy/MM/dz}");
        assert_eq!(messages.len(), 1, "{:?}", messages);
        assert!(messages[0].contains("書式ではない英字"), "{:?}", messages);
    }

    /// 書式の間違いは黙って誤ったファイル名を作るので、警告ではなくエラーにする
    #[test]
    fn 日時の書式の間違いはメニューを止める() {
        let parsed = parse("[.txt]\nA | C:\\Windows\\notepad.exe | $t{");
        assert!(parsed.has_error());
    }

    #[test]
    fn 正しい日時の書式は通る() {
        for args in [
            "$-p_$t{yyyyMMdd}.zip",
            "$t{yyyy年MM月dd日(ddd)}",
            "-o $t{yyyy-MM-dd_HHmmss} $p",
            // エスケープしたものは書式ではない
            "^$t{これは書式ではない}",
            // 中括弧を伴わない $t は今までどおり素通りする
            "$t",
        ] {
            let text = format!("[.txt]\nA | C:\\Windows\\notepad.exe | {}", args);
            assert!(errors(&text).is_empty(), "{}", args);
        }
    }
}

fn split_args(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    // `$t{...}` と `$?{...}` の中括弧の深さ。中の空白では区切らない
    let mut braces = 0usize;
    let mut i = 0;

    while i < bytes.len() {
        if let Some(len) = escape_len(bytes, i) {
            current.push_str(&text[i..i + len]);
            started = true;
            i += len;
            continue;
        }

        match bytes[i] {
            b'"' => {
                quoted = !quoted;
                started = true;
                i += 1;
            }
            // 中括弧を数えるのは `$t{` と `$?{` で開いたものだけ。素の `{` まで
            // 数えると、PowerShell のスクリプトブロックが引数をまたいで繋がる
            b'$' if bytes[i + 1..].starts_with(b"t{") || bytes[i + 1..].starts_with(b"?{") => {
                current.push_str(&text[i..i + 3]);
                braces += 1;
                started = true;
                i += 3;
            }
            b'}' if braces > 0 => {
                current.push('}');
                braces -= 1;
                started = true;
                i += 1;
            }
            b' ' | b'\t' if !quoted && braces == 0 => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
                i += 1;
            }
            b => {
                let len = char_len(b);
                current.push_str(&text[i..i + len]);
                started = true;
                i += len;
            }
        }
    }

    if started {
        args.push(current);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(text: &str) -> Config {
        let parsed = parse(text);
        let errors: Vec<String> = parsed
            .errors()
            .map(|d| format!("{}行目: {}", d.line, d.message))
            .collect();
        assert!(errors.is_empty(), "予期しないエラー: {:?}", errors);
        parsed.config
    }

    fn error_messages(text: &str) -> Vec<String> {
        parse(text).errors().map(|d| d.message.clone()).collect()
    }

    fn warning_messages(text: &str) -> Vec<String> {
        parse(text)
            .diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .map(|d| d.message.clone())
            .collect()
    }

    #[test]
    fn 基本的な項目をパースできる() {
        let config = parse_ok("[.txt]\nメモ帳 | C:\\notepad.exe | -x $p");
        assert_eq!(config.apps.len(), 1);
        let item = &config.apps[0];
        assert_eq!(item.name, "メモ帳");
        assert_eq!(item.path, "C:\\notepad.exe");
        assert_eq!(item.args, vec!["-x", "$p"]);
        assert_eq!(item.extensions, vec![".txt"]);
        assert!(!item.all_mode);
        assert_eq!(item.line, 2);
    }

    // -----------------------------------------------------------------
    // グローバル設定（[extrun]）
    // -----------------------------------------------------------------

    #[test]
    fn 設定セクションを読める() {
        let config = parse_ok("[extrun]\nmenu-position = window\nselect-first = yes");
        assert_eq!(config.settings.menu_position, MenuPosition::Window);
        assert!(config.settings.select_first);
    }

    #[test]
    fn 設定を書かなければ既定値() {
        let config = parse_ok("[.txt]\nA | C:\\a.exe");
        assert_eq!(config.settings.menu_position, MenuPosition::Cursor);
        assert!(!config.settings.select_first);
    }

    #[test]
    fn 設定セクションはメニュー項目を作らない() {
        let config = parse_ok("[extrun]\nmenu-position = screen\n[.txt]\nA | C:\\a.exe");
        assert_eq!(config.apps.len(), 1);
        assert_eq!(config.apps[0].name, "A");
    }

    /// 拡張子セクションと違い、設定はどこに書いても全体に効く
    #[test]
    fn 設定セクションはファイルのどこに書いてもよい() {
        let config = parse_ok("[.txt]\nA | C:\\a.exe\n[extrun]\nselect-first = yes");
        assert!(config.settings.select_first);
        assert_eq!(config.apps.len(), 1);
    }

    /// 設定セクションのあとに拡張子セクションが来たら項目に戻る
    #[test]
    fn 拡張子セクションで設定セクションを抜ける() {
        let config = parse_ok("[extrun]\nselect-first = yes\n[.txt]\nメモ帳 | C:\\a.exe");
        assert!(config.settings.select_first);
        assert_eq!(config.apps.len(), 1);
        assert_eq!(config.apps[0].name, "メモ帳");
    }

    #[test]
    fn 表示位置の値を解釈する() {
        assert_eq!(parse_menu_position("cursor"), Some(MenuPosition::Cursor));
        assert_eq!(parse_menu_position("window"), Some(MenuPosition::Window));
        assert_eq!(parse_menu_position("screen"), Some(MenuPosition::Screen));
        assert_eq!(parse_menu_position(" SCREEN "), Some(MenuPosition::Screen));
        assert_eq!(
            parse_menu_position("100,200"),
            Some(MenuPosition::Point { x: 100, y: 200 })
        );
        assert_eq!(
            parse_menu_position("100, 200"),
            Some(MenuPosition::Point { x: 100, y: 200 })
        );
        // 主モニタより左や上にある画面の座標は負になる
        assert_eq!(
            parse_menu_position("-1920,-100"),
            Some(MenuPosition::Point { x: -1920, y: -100 })
        );
    }

    #[test]
    fn 表示位置の不正な値は読めない() {
        for value in ["", "middle", "100", "100,", "100,abc", "100,200,300"] {
            assert_eq!(parse_menu_position(value), None, "{}", value);
        }
    }

    #[test]
    fn 未知の設定はエラー() {
        let errors = error_messages("[extrun]\nmenu-postion = window");
        assert!(
            errors.iter().any(|m| m.contains("未知の設定")),
            "{:?}",
            errors
        );
    }

    #[test]
    fn 不正な設定値はエラー() {
        let errors = error_messages("[extrun]\nmenu-position = middle");
        assert!(
            errors
                .iter()
                .any(|m| m.contains("menu-position の値が不正")),
            "{:?}",
            errors
        );

        let errors = error_messages("[extrun]\nselect-first = maybe");
        assert!(
            errors.iter().any(|m| m.contains("select-first の値が不正")),
            "{:?}",
            errors
        );
    }

    #[test]
    fn 設定の重複はエラー() {
        let errors = error_messages("[extrun]\nmenu-position = window\nmenu-position = screen");
        assert!(
            errors.iter().any(|m| m.contains("設定が重複しています")),
            "{:?}",
            errors
        );
    }

    #[test]
    fn イコールのない設定行はエラー() {
        let errors = error_messages("[extrun]\nmenu-position window");
        assert!(
            errors.iter().any(|m| m.contains("名前 = 値")),
            "{:?}",
            errors
        );
    }

    #[test]
    fn 設定セクションの中の_dir_はエラー() {
        let errors = error_messages("[extrun]\nmenu-position = window\n :dir C:\\tmp");
        assert!(
            errors.iter().any(|m| m.contains(":dir を使えません")),
            "{:?}",
            errors
        );
    }

    // -----------------------------------------------------------------
    // アクセスキー
    // -----------------------------------------------------------------

    #[test]
    fn アクセスキーを名前から取り出す() {
        let config = parse_ok("[.txt]\n開く (&O) | C:\\a.exe");
        let item = &config.apps[0];
        // 目印の `&` は表示名から落ちる
        assert_eq!(item.name, "開く (O)");
        assert_eq!(item.accesskey_char(), Some('O'));
    }

    #[test]
    fn 名前の先頭のアンパサンドもアクセスキーになる() {
        let config = parse_ok("[.txt]\n&PNG に変換 | C:\\a.exe");
        assert_eq!(config.apps[0].name, "PNG に変換");
        assert_eq!(config.apps[0].accesskey, Some(0));
        assert_eq!(config.apps[0].accesskey_char(), Some('P'));
    }

    #[test]
    fn アクセスキーは大文字に正規化される() {
        let config = parse_ok("[.txt]\n開く (&o) | C:\\a.exe");
        assert_eq!(config.apps[0].name, "開く (o)");
        assert_eq!(config.apps[0].accesskey_char(), Some('O'));
    }

    #[test]
    fn エスケープしたアンパサンドはアクセスキーにならない() {
        let config = parse_ok("[.txt]\nQ^&A のかたち | C:\\a.exe");
        assert_eq!(config.apps[0].name, "Q&A のかたち");
        assert_eq!(config.apps[0].accesskey, None);
    }

    /// `&` の直後が半角英数字でなければ、ただの `&` として残す
    #[test]
    fn アンパサンドの後ろが英数字でなければただの文字() {
        let config = parse(("[.txt]\nA & B | C:\\a.exe").trim()).config;
        assert_eq!(config.apps[0].name, "A & B");
        assert_eq!(config.apps[0].accesskey, None);
    }

    #[test]
    fn アクセスキーは最初のひとつだけ() {
        let config = parse("[.txt]\n&A と &B | C:\\a.exe").config;
        assert_eq!(config.apps[0].accesskey_char(), Some('A'));
        // 2 つ目の `&` はただの文字として残る
        assert_eq!(config.apps[0].name, "A と &B");
    }

    #[test]
    fn セパレーターはアクセスキーを持たない() {
        let config = parse_ok("[.txt]\n---");
        assert!(config.apps[0].is_separator());
        assert_eq!(config.apps[0].accesskey, None);
    }

    #[test]
    fn 別名の中のアクセスキーも解決される() {
        let config = parse_ok("@n = 開く (&O)\n[.txt]\n@n | C:\\a.exe");
        assert_eq!(config.apps[0].name, "開く (O)");
        assert_eq!(config.apps[0].accesskey_char(), Some('O'));
    }

    /// アクセスキーは名前欄だけの記法。引数の `&` はそのまま渡す
    /// （PowerShell の呼び出し演算子で使う）
    #[test]
    fn 引数のアンパサンドはそのまま残る() {
        let config = parse_ok("[.txt]\nA | C:\\a.exe | -Command \"& 'C:\\b.exe'\"");
        assert_eq!(config.apps[0].args, vec!["-Command", "& 'C:\\b.exe'"]);
    }

    #[test]
    fn アクセスキーの書き間違いを警告する() {
        let warnings = warning_messages("[.txt]\nA & B | C:\\a.exe");
        assert!(
            warnings.iter().any(|m| m.contains("後ろは半角英数字")),
            "{:?}",
            warnings
        );

        let warnings = warning_messages("[.txt]\n&A と &B | C:\\a.exe");
        assert!(
            warnings.iter().any(|m| m.contains("& が複数あります")),
            "{:?}",
            warnings
        );
    }

    #[test]
    fn 正しいアクセスキーは警告しない() {
        for name in ["開く (&O)", "&PNG に変換", "Q^&A のかたち"] {
            let warnings = warning_messages(&format!("[.txt]\n{} | C:\\a.exe", name));
            assert!(warnings.is_empty(), "{}: {:?}", name, warnings);
        }
    }

    #[test]
    fn 引数を省略すると_p_が既定になる() {
        let config = parse_ok("[.txt]\nメモ帳 | C:\\notepad.exe");
        assert_eq!(config.apps[0].args, vec!["$p"]);
    }

    #[test]
    fn 継続行が連結される() {
        let one_line = parse_ok("[.jpg]\n最適化 | C:\\a.exe | -copy none -outfile $p $p");
        let multi_line = parse_ok("[.jpg]\n最適化\n | C:\\a.exe\n | -copy none -outfile $p $p");
        assert_eq!(multi_line.apps[0].name, one_line.apps[0].name);
        assert_eq!(multi_line.apps[0].path, one_line.apps[0].path);
        assert_eq!(multi_line.apps[0].args, one_line.apps[0].args);
        // 行番号は最初の物理行を保つ
        assert_eq!(multi_line.apps[0].line, 2);
    }

    #[test]
    fn 行頭のパイプはエスケープすると継続行にならない() {
        let config = parse_ok("[.txt]\n名前 | C:\\a.exe\n^| 縦棒 | C:\\b.exe");
        assert_eq!(config.apps.len(), 2);
        assert_eq!(config.apps[1].name, "| 縦棒");
    }

    #[test]
    fn 行頭マーカーでサブメニューを作る() {
        let config = parse_ok(
            "[.txt]\n圧縮\n> CBZ\n>> 個別 | C:\\a.exe\n> + まとめて | C:\\b.exe\nその他 | C:\\c.exe",
        );
        assert_eq!(config.apps.len(), 2);
        let compress = &config.apps[0];
        assert_eq!(compress.name, "圧縮");
        assert_eq!(compress.submenu.len(), 2);
        assert_eq!(compress.submenu[0].name, "CBZ");
        assert_eq!(compress.submenu[0].submenu.len(), 1);
        assert_eq!(compress.submenu[0].submenu[0].name, "個別");
        assert_eq!(compress.submenu[1].name, "まとめて");
        assert!(compress.submenu[1].all_mode);
        assert_eq!(config.apps[1].name, "その他");
    }

    #[test]
    fn 空白が続かないマーカーは名前の一部になる() {
        let config = parse_ok("[.txt]\n+新規作成 | C:\\a.exe\n>>移動 | C:\\b.exe");
        assert_eq!(config.apps.len(), 2);
        assert_eq!(config.apps[0].name, "+新規作成");
        assert!(!config.apps[0].all_mode);
        assert_eq!(config.apps[1].name, ">>移動");
    }

    #[test]
    fn 名前の途中の記号はエスケープ不要() {
        let config = parse_ok("[.jpg]\nJpeg > PNG に変換 | C:\\a.exe\n画像 + 動画 | C:\\b.exe");
        assert_eq!(config.apps[0].name, "Jpeg > PNG に変換");
        assert_eq!(config.apps[1].name, "画像 + 動画");
    }

    #[test]
    fn 拡張子は継承する() {
        let config = parse_ok("[.flac .wav]\n変換 | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec![".flac", ".wav"]);
    }

    #[test]
    fn 拡張子を引き算できる() {
        let config = parse_ok("[.jpg .png .svg]\n変換 [-.jpg -.svg] | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec![".png"]);
    }

    #[test]
    fn 拡張子を完全置換できる() {
        let config = parse_ok("[.jpg .png]\n変換 [.svg] | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec![".svg"]);
    }

    #[test]
    fn 子項目は親の拡張子を継承する() {
        let config = parse_ok("[.jpg .png .svg]\n変換 [-.svg]\n> Jpeg [-.jpg] | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec![".jpg", ".png"]);
        assert_eq!(config.apps[0].submenu[0].extensions, vec![".png"]);
    }

    #[test]
    fn 拡張子は小文字化される() {
        let config = parse_ok("[.JPG]\n変換 [.PNG] | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec![".png"]);
    }

    #[test]
    fn セパレーターは拡張子を持てる() {
        let config = parse_ok("[file]\nA | C:\\a.exe\n--- [folder .mp3]\nB | C:\\b.exe");
        assert!(config.apps[1].is_separator());
        assert_eq!(config.apps[1].extensions, vec!["folder", ".mp3"]);
    }

    #[test]
    fn サブメニュー内のセパレーター() {
        let config = parse_ok("[file]\n親\n> A | C:\\a.exe\n> ---\n> B | C:\\b.exe");
        assert_eq!(config.apps[0].submenu.len(), 3);
        assert!(config.apps[0].submenu[1].is_separator());
    }

    #[test]
    fn 別名を展開する() {
        let config = parse_ok(
            "@tools = C:\\Tools\n@editor = @tools\\editor\\editor.exe\n[.txt]\n編集 | @editor | $p",
        );
        assert_eq!(config.apps[0].path, "C:\\Tools\\editor\\editor.exe");
    }

    #[test]
    fn 別名は後から定義してもよい() {
        let config = parse_ok("[.txt]\n編集 | @editor\n@editor = C:\\Tools\\editor.exe");
        assert_eq!(config.apps[0].path, "C:\\Tools\\editor.exe");
    }

    #[test]
    fn 別名を拡張子欄で使える() {
        let config = parse_ok("@画像 = .png .jpg\n[@画像]\n変換 | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec![".png", ".jpg"]);
    }

    #[test]
    fn 別名はパスの途中でも展開される() {
        let config =
            parse_ok("@tools = C:\\Tools\n[.txt]\nA | @tools\\sub\\a.exe | @tools\\x.py $p");
        assert_eq!(config.apps[0].path, "C:\\Tools\\sub\\a.exe");
        assert_eq!(config.apps[0].args, vec!["C:\\Tools\\x.py", "$p"]);
    }

    #[test]
    fn エスケープした別名は展開されない() {
        let config = parse_ok("@f = X\n[.txt]\nA | C:\\a.exe | ^@filelist.txt");
        // 引数は実行時に解決するため ^ が残る
        assert_eq!(config.apps[0].args, vec!["^@filelist.txt"]);
    }

    #[test]
    fn 名前のエスケープはパース時に解決される() {
        let config = parse_ok("[.txt]\n^> 引用 ^| 記号 | C:\\a.exe");
        assert_eq!(config.apps[0].name, "> 引用 | 記号");
    }

    #[test]
    fn 引数のエスケープはパース時に解決しない() {
        let config = parse_ok("[.txt]\nA | C:\\a.exe | ^$HOME $p");
        assert_eq!(config.apps[0].args, vec!["^$HOME", "$p"]);
    }

    #[test]
    fn 引用符で空白を含む引数を書ける() {
        let config = parse_ok("[.txt]\nA | C:\\a.exe | -title \"My File\" $p");
        assert_eq!(config.apps[0].args, vec!["-title", "My File", "$p"]);
    }

    #[test]
    fn 作業フォルダを指定できる() {
        let config = parse_ok(
            "[folder]\nエクスプローラ | C:\\Explorer.EXE | $p\n :dir C:\\WINDOWS\\system32\\",
        );
        assert_eq!(config.apps[0].working_dir, "C:\\WINDOWS\\system32\\");
    }

    #[test]
    fn コメントと空行は無視される() {
        let config = parse_ok("# コメント\n\n[.txt]\n\n# 別のコメント\nA | C:\\a.exe");
        assert_eq!(config.apps.len(), 1);
    }

    #[test]
    fn 行の途中のシャープはただの文字() {
        let config = parse_ok("[.txt]\nA # B | C:\\a.exe");
        assert_eq!(config.apps[0].name, "A # B");
    }

    #[test]
    fn bom付きでも読める() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("[.txt]\nA | C:\\a.exe".as_bytes());
        let text = decode_utf8(&bytes).expect("UTF-8 として読める");
        let config = parse_ok(&text);
        assert_eq!(config.apps[0].name, "A");
    }

    #[test]
    fn 不正なutf8は拒否される() {
        assert!(decode_utf8(&[0x82, 0xA0]).is_none());
    }

    #[test]
    fn 重複した別名はエラー() {
        let errors = error_messages("@a = 1\n@a = 2\n[.txt]\nX | @a");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("重複"));
    }

    #[test]
    fn 未定義の別名はエラー() {
        let errors = error_messages("[.txt]\nX | @undefined");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("未定義の別名 @undefined"));
    }

    #[test]
    fn 循環参照はエラー() {
        let errors = error_messages("@a = @b\n@b = @a\n[.txt]\nX | @a");
        assert!(errors.iter().any(|m| m.contains("循環参照")));
    }

    #[test]
    fn 使われていない別名の循環も検出する() {
        let errors = error_messages("@a = @b\n@b = @a\n[.txt]\nX | C:\\x.exe");
        assert!(errors.iter().any(|m| m.contains("循環参照")));
    }

    #[test]
    fn 別名の定義に含まれる未定義の別名も検出する() {
        let errors = error_messages("@a = @typo\\bin\n[.txt]\nX | C:\\x.exe");
        assert!(errors.iter().any(|m| m.contains("未定義の別名 @typo")));
    }

    #[test]
    fn 拡張子の混在はエラー() {
        let errors = error_messages("[.jpg .png]\nX [-.jpg .svg] | C:\\a.exe");
        assert!(errors.iter().any(|m| m.contains("混在")));
    }

    #[test]
    fn セクション見出しの引き算はエラー() {
        let errors = error_messages("[-.jpg]\nX | C:\\a.exe");
        assert!(errors.iter().any(|m| m.contains("引き算")));
    }

    #[test]
    fn 先頭のドットがない拡張子はエラー() {
        let errors = error_messages("[jpg]\nX | C:\\a.exe");
        assert!(errors.iter().any(|m| m.contains("先頭の . がありません")));
    }

    #[test]
    fn file_と_folder_はドット不要() {
        let config = parse_ok("[file folder]\nX | C:\\a.exe");
        assert_eq!(config.apps[0].extensions, vec!["file", "folder"]);
    }

    #[test]
    fn セクション見出しがないとエラー() {
        let errors = error_messages("X | C:\\a.exe");
        assert!(errors.iter().any(|m| m.contains("セクション見出し")));
    }

    #[test]
    fn 未知のキーワードはエラー() {
        let errors = error_messages("[.txt]\nX | C:\\a.exe\n :cwd C:\\");
        assert!(errors.iter().any(|m| m.contains("未知のキーワード")));
    }

    #[test]
    fn 階層が飛んでいるとエラー() {
        let errors = error_messages("[.txt]\n>> X | C:\\a.exe");
        assert!(errors.iter().any(|m| m.contains("階層")));
    }

    #[test]
    fn フィールドが多すぎるとエラー() {
        let errors = error_messages("[.txt]\nA | B | C | D");
        assert!(errors.iter().any(|m| m.contains("フィールドが多すぎます")));
    }

    #[test]
    fn エラーには行番号が付く() {
        let parsed = parse("[.txt]\nA | C:\\a.exe\n\nB | @nothing");
        let error = parsed.errors().next().expect("エラーがある");
        assert_eq!(error.line, 4);
    }

    #[test]
    fn 宙に浮いたキャレットは警告() {
        let warnings = warning_messages("[.txt]\nA | C:\\Foo^Bar\\app.exe");
        assert!(warnings.iter().any(|m| m.contains("^")));
    }

    #[test]
    fn 正しいエスケープは警告しない() {
        let warnings = warning_messages("[.txt]\n^+ A | C:\\a.exe | ^$p");
        assert!(warnings.is_empty(), "{:?}", warnings);
    }
}
