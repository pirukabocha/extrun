/*!
設定ファイルの文字列を扱う部品

エスケープ（`^X`）の解決、区切り文字での分割、`%NAME%` の展開、引数の分解。
**パーサ（`config.rs`）と実行時の置換（`placeholder.rs`）の両方から使う**ので、
どちらか片方の都合で振る舞いを変えないこと。エスケープの決まりが 2 か所に
分かれていた頃は、片方だけを直して `^X` が実行時に素通りする事故が起きた。
*/

/// エスケープ（`^`）の対象になる特殊文字
///
/// `placeholder.rs` も同じ定義を使う。引数のエスケープはパース時ではなく
/// 実行時に解決されるので、2 か所で食い違うと `^X` が片方だけ素通りする。
pub(crate) const SPECIALS: &[u8] = b"^@$|:>+-#[]&";

/// BOM を読み飛ばして UTF-8 として解釈する
pub(crate) fn decode_utf8(bytes: &[u8]) -> Option<String> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(body.to_vec()).ok()
}

/// エスケープの対象になる文字か
pub(crate) fn is_special(b: u8) -> bool {
    SPECIALS.contains(&b)
}

/// UTF-8 の 1 文字のバイト数
pub(crate) fn char_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// `^X`（X は特殊文字）ならその長さを返す
pub(crate) fn escape_len(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes[i] == b'^' && i + 1 < bytes.len() && is_special(bytes[i + 1]) {
        Some(2)
    } else {
        None
    }
}

/// エスケープされていない区切り文字で分割する
pub(crate) fn split_unescaped(text: &str, sep: u8) -> Vec<&str> {
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
pub(crate) fn unescape(text: &str) -> String {
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

/// 名前欄に書ける形にする（`unescape_name` の逆）
///
/// **設定ファイルを組み立てる `extrun-make` のためのもの。** ExtRun 本体は
/// 読む側なので使わない。同じファイルに置いてあるのは、`SPECIALS` の定義から
/// 離すと「片方だけ直す」事故がまた起きるから。
///
/// 掛けるのは**その欄で実際に意味を持つ文字だけ**にしてある。`+` や `-` や
/// `#` まで機械的に潰すと「C++ で開く」が `C^+^+ で開く` になり、貼り付ける人が
/// 読めなくなる（このツールの目的からすると本末転倒）。
///
/// - どこにあっても意味を持つ: `^`（エスケープ）/ `@`（別名）/ `|`（欄の区切り）
///   / `&`（アクセスキー）/ `[` `]`（項目ごとの対象指定）
/// - **行頭にあるときだけ**意味を持つ: `>` `+` `-` `#` `:`（行頭マーカーと
///   コメントと名前付きフィールド）
///
/// 正しさは字面ではなく**実際のパーサを通した往復**で担保する
/// （`extrun-make` 側に、組み立て → `config::parse` → 名前が一致、のテストがある）。
pub fn escape_name(text: &str) -> String {
    escape_with(text, b"^@|&[]", b">+-#:")
}

/// パス欄（実行ファイル・`:icon`）に書ける形にする
///
/// パス欄は `unescape` を通るだけで、アクセスキーも行頭マーカーも見られない。
/// `&` や `+` を含むフォルダ名をそのまま書けるよう、対象は 5 つに絞る。
///
/// **`%` は対象外**。`%NAME%` の展開はパス欄の機能なので、潰すと
/// `%LOCALAPPDATA%` が書けなくなる。
pub fn escape_path(text: &str) -> String {
    escape_with(text, b"^@|[]", b"")
}

/// `always` の文字は全部、`leading` の文字は先頭にあるときだけ `^` を付ける
///
/// 「先頭」は**空白を読み飛ばした最初の文字**。パーサが行頭の空白を無視するので、
/// 位置で数えると ` > 開く` のような名前がマーカーとして読まれてしまう。
fn escape_with(text: &str, always: &[u8], leading: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_start = true;

    for ch in text.chars() {
        let byte = if ch.is_ascii() { ch as u8 } else { 0 };

        if always.contains(&byte) || (at_start && leading.contains(&byte)) {
            out.push('^');
        }
        out.push(ch);

        if !ch.is_whitespace() {
            at_start = false;
        }
    }

    out
}

/// `%NAME%` を環境変数の値に置き換える（パス用）
///
/// パスを書く欄でだけ使う。引数欄で展開すると `cmd.exe /c` に渡す
/// `%errorlevel%` や `%~n1` を横取りしてしまうため。
///
/// **パース時に済ませる**のが肝で、実行時に展開すると `100%OFF.png` のような
/// 対象ファイルの名前に含まれる `%` まで置換の対象になる。ユーザーのデータを
/// 設定として解釈しないための線引き。
///
/// Windows の `ExpandEnvironmentStrings` に挙動を合わせてある。未定義の変数は
/// `%NAME%` のまま残し（`--check` の「実行ファイルが見つかりません」に展開前の
/// 文字列が出るので、そこで気づける）、`%%` は特別扱いしない。名前の大文字小文字
/// は区別しない（Windows の環境変数の決まりで、`std::env` もそう振る舞う）。
pub(crate) fn expand_env(text: &str, keep_escapes: bool) -> String {
    if !text.contains('%') {
        return text.to_string();
    }

    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'%' {
            let len = char_len(bytes[i]);
            out.push_str(&text[i..i + len]);
            i += len;
            continue;
        }

        // 閉じの `%` を探す。空の名前（`%%`）と閉じ忘れはそのまま通す
        match text[i + 1..].find('%').map(|pos| i + 1 + pos) {
            Some(close) if close > i + 1 => {
                let name = &text[i + 1..close];
                match std::env::var_os(name) {
                    Some(value) => {
                        let value = value.to_string_lossy();
                        if keep_escapes {
                            // 実行時にエスケープを解決する欄では、展開値の中の `^`
                            // が目印として食われないように二重化する
                            out.push_str(&value.replace('^', "^^"));
                        } else {
                            out.push_str(&value);
                        }
                    }
                    None => out.push_str(&text[i..=close]),
                }
                i = close + 1;
            }
            _ => {
                out.push('%');
                i += 1;
            }
        }
    }

    out
}

/// 展開されなかった `%NAME%` が残っているか
///
/// 展開はパース時に済んでいるので、ここで見つかるのは未定義の変数（または
/// パスに含まれる素の `%`）。**`%FOO%\app.exe` は絶対パスと判定されない**ため、
/// `--check` の存在確認は絶対パスかどうかに加えてこれを見る。見ないと、パスの
/// 先頭に書いた変数の綴りを間違えたときに何の警告も出ない。
pub fn has_unexpanded_env(text: &str) -> bool {
    let mut rest = text;

    while let Some(open) = rest.find('%') {
        let after = &rest[open + 1..];
        match after.find('%') {
            // `%%` は変数ではない
            Some(0) => rest = &after[1..],
            Some(_) => return true,
            None => return false,
        }
    }

    false
}

/// アクセスキーになれる文字か（半角英数字）
pub(crate) fn is_accesskey_char(b: Option<&u8>) -> bool {
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
/// （`config::warn_accesskey` が別に警告する）。
pub(crate) fn unescape_name(text: &str) -> (String, Option<usize>) {
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

/// エスケープされていない `$p` を含むか（`$-p` は別のプレースホルダーなので除く）
///
/// **`--check` の警告（`check::warn_embedded_path_placeholder`）と、`+` の引数を
/// 組み立てる `invoke::all_mode_args` の両方がこれを使う。** かつては後者だけが
/// 素の `contains("$p")` で判定していて、`^$path` のようにエスケープした `$` を
/// 書くと「`$p` がある」と誤解し、対象のパスがどこにも渡らないまま起動していた。
/// しかも `--check` 側は正しく読めていたので警告も出なかった。判定を 2 か所に
/// 分けると必ずこうなるので、ここ 1 か所に置く。
pub(crate) fn has_path_placeholder(arg: &str) -> bool {
    let bytes = arg.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // `^$` はプレースホルダーではない
        if bytes[i] == b'^' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'p') {
            return true;
        }
        i += 1;
    }

    false
}

/// 中括弧を開くプレースホルダー（`$t{` / `$?{` / `$?int{` …）の長さ
fn placeholder_open_len(text: &str, i: usize) -> Option<usize> {
    if text.as_bytes()[i..].starts_with(b"$t{") {
        return Some(3);
    }
    crate::prompt::opening_len(text, i)
}

/// 引数を空白区切りで分解する（引用符で空白を含められる）
pub(crate) fn split_args(text: &str) -> Vec<String> {
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

        // 中括弧を数えるのは `$t{` と `$?...{` で開いたものだけ。素の `{` まで
        // 数えると、PowerShell のスクリプトブロックが引数をまたいで繋がる
        if let Some(len) = placeholder_open_len(text, i) {
            current.push_str(&text[i..i + len]);
            braces += 1;
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
            // **中括弧の中でだけ、素の `{` も数える。** `$?{PowerShell の {0}}`
            // のように説明へ中括弧を書いても対応が取れ、引数が途中で切れない。
            // `prompt::find_close`（`$?{...}` の終端を探す側）はすべての `{` を
            // 数えるので、ここで数えないと二者の見解が食い違い、「閉じられて
            // いません」という見当違いのエラーになっていた。
            //
            // 深さ 0 では数えないままなので、PowerShell のスクリプトブロックが
            // 引数をまたいで繋がることはない（下の「素の中括弧は数えない」）。
            b'{' if braces > 0 => {
                current.push('{');
                braces += 1;
                started = true;
                i += 1;
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

    /// 中括弧の中でだけ素の `{` も数える。`prompt::find_close`（終端を探す側）と
    /// 数え方が食い違うと、引数が途中で切れたうえ「閉じられていません」という
    /// 見当違いのエラーが出る
    #[test]
    fn 中括弧の中の素の中括弧も数える() {
        assert_eq!(
            split_args("$?{PowerShell の {0} 形式で} -f"),
            vec!["$?{PowerShell の {0} 形式で}", "-f"]
        );
    }

    /// `^$` はエスケープなのでプレースホルダーではない。ここを素の
    /// `contains("$p")` で判定すると、`+` の項目でパスがどこにも渡らなくなる
    #[test]
    fn エスケープした_p_はプレースホルダーではない() {
        assert!(has_path_placeholder("$p"));
        assert!(has_path_placeholder("-i$p"));
        assert!(!has_path_placeholder("^$path"));
        // `$-p` は別のプレースホルダー
        assert!(!has_path_placeholder("$-p"));
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

    #[test]
    fn 展開されなかった環境変数を見分ける() {
        assert!(has_unexpanded_env("%FOO%\\a.exe"));
        assert!(has_unexpanded_env("C:\\dir\\%FOO%\\a.exe"));
        assert!(!has_unexpanded_env("C:\\dir\\a.exe"));
        // 素の `%` と `%%` は変数ではない
        assert!(!has_unexpanded_env("C:\\100%\\a.exe"));
        assert!(!has_unexpanded_env("C:\\50%%off\\a.exe"));
    }

    #[test]
    fn bomを読み飛ばす() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("あ".as_bytes());
        assert_eq!(decode_utf8(&bytes).as_deref(), Some("あ"));
    }

    #[test]
    fn 不正なutf8は拒否される() {
        assert!(decode_utf8(&[0x82, 0xA0]).is_none());
    }

    // --- エスケープを掛ける方向（extrun-make 用） ---

    #[test]
    fn 名前欄でどこにあっても潰す文字() {
        assert_eq!(escape_name("Q&A"), "Q^&A");
        assert_eq!(escape_name("a|b"), "a^|b");
        assert_eq!(escape_name("mail@例"), "mail^@例");
        assert_eq!(escape_name("^"), "^^");
        assert_eq!(escape_name("[1]"), "^[1^]");
    }

    /// 「C++ で開く」が「C^+^+ で開く」になっては、貼る人が読めない
    #[test]
    fn 名前欄の途中の記号はそのまま残す() {
        assert_eq!(escape_name("C++ で開く"), "C++ で開く");
        assert_eq!(escape_name("PNG - JPEG"), "PNG - JPEG");
        assert_eq!(escape_name("注意: 上書き"), "注意: 上書き");
        assert_eq!(escape_name("ページ #1 を開く"), "ページ #1 を開く");
    }

    #[test]
    fn 名前欄の行頭の記号は潰す() {
        assert_eq!(escape_name("> 開く"), "^> 開く");
        assert_eq!(escape_name("+ まとめる"), "^+ まとめる");
        assert_eq!(escape_name("---"), "^---");
        assert_eq!(escape_name("# コメント"), "^# コメント");
        assert_eq!(escape_name(":dir"), "^:dir");
    }

    /// パーサは行頭の空白を読み飛ばすので、位置で数えるとマーカーが素通りする
    #[test]
    fn 空白の後ろも行頭とみなす() {
        assert_eq!(escape_name("  > 開く"), "  ^> 開く");
    }

    #[test]
    fn パス欄はアクセスキーも行頭マーカーも見ない() {
        assert_eq!(
            escape_path(r"C:\Program Files (x86)\a&b\x.exe"),
            r"C:\Program Files (x86)\a&b\x.exe"
        );
        assert_eq!(escape_path(r"C:\dir-1\+new\x.exe"), r"C:\dir-1\+new\x.exe");
        assert_eq!(escape_path(r"C:\a^b\x.exe"), r"C:\a^^b\x.exe");
        assert_eq!(escape_path(r"C:\a@b\x.exe"), r"C:\a^@b\x.exe");
    }

    /// `%LOCALAPPDATA%` が書けなくなるので `%` は対象にしない
    #[test]
    fn パス欄の環境変数は潰さない() {
        assert_eq!(
            escape_path(r"%LOCALAPPDATA%\app.exe"),
            r"%LOCALAPPDATA%\app.exe"
        );
    }

    /// 掛けたものは必ず解ける（`unescape` は `SPECIALS` 全部を戻す）
    #[test]
    fn 掛けたエスケープは解いて元に戻る() {
        let 元 = ["Q&A", "C++ で開く", "> 開く", "^@|[]&", "---", "a|b^c@d"];
        for text in 元 {
            assert_eq!(unescape(&escape_name(text)), text, "{}", text);
            assert_eq!(unescape(&escape_path(text)), text, "{}", text);
        }
    }

    /// 名前欄はアクセスキーの読み取りも通るので、そちらでも元に戻る必要がある
    #[test]
    fn 名前欄はアクセスキーを持たない形に戻る() {
        let (名前, キー) = unescape_name(&escape_name("Q&A"));
        assert_eq!(名前, "Q&A");
        assert_eq!(キー, None);
    }
}
