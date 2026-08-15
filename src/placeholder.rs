/*!
プレースホルダーの置換処理

引数と作業フォルダはパース時に `^` を残したまま渡ってくる。ここで左から 1 回
走査して `^X` → `X` と `$x` → 置換値 を同時に処理する。先にエスケープだけを
解決すると `^$` が `$` になったあとプレースホルダーとして拾われてしまう。
*/

use crate::config::SPECIALS;
use crate::datetime::LocalTime;
use std::cell::RefCell;
use std::path::Path;

/// 対象のパスから導けない、実行時に決まる値
///
/// パス由来の値（`PathPlaceholders`）が対象ごとに作り直されるのに対して、
/// こちらは**対象をまたいで共有する**。日時は起動の直前に 1 回だけ確定させる。
/// 対象ごとに取り直すと、複数選択して個別に起動したときに `$t{ss}` がずれて、
/// まとめて作ったはずのファイル名が揃わなくなる。
///
/// `$?{...}` の答えも同じ理由でここに置く。こちらは**対象ごとに聞かれては
/// 困る**という、より強い理由がある。`replace()` は対象の数だけ呼ばれるので、
/// 答えを持たずに毎回聞くと入力欄が何度も出てしまう。
pub struct RunContext {
    /// `$t{...}` が使う時刻
    pub now: LocalTime,
    /// `$?{...}` の答え
    ///
    /// 見出しは `$?int{長辺=1280}` のように**書かれたとおりの文字列全体**。
    /// 決まりの語まで含めないと、`$?{幅}` と `$?int{幅}` が同じ答えを共有する。
    ///
    /// `replace()` は `&self` で呼ばれるので `RefCell` で包む。答えを入れるのは
    /// 置換が始まる前だけなので、借用が重なることはない。
    prompts: RefCell<Vec<(String, String)>>,
}

impl RunContext {
    /// 実行時の値をここで確定させる
    pub fn capture() -> Self {
        RunContext {
            now: LocalTime::now(),
            prompts: RefCell::new(Vec::new()),
        }
    }

    /// 時刻を固定した実行時コンテキスト（2026-08-15 土曜 14:03:05）
    #[cfg(test)]
    pub fn for_test() -> Self {
        RunContext {
            now: crate::datetime::test_time(),
            prompts: RefCell::new(Vec::new()),
        }
    }

    /// `$?{...}` の答えを引く
    pub fn prompt_value(&self, spec: &str) -> Option<String> {
        self.prompts
            .borrow()
            .iter()
            .find(|(key, _)| key == spec)
            .map(|(_, value)| value.clone())
    }

    /// `$?{...}` の答えを覚える（同じ内容があれば置き換える）
    pub fn set_prompt(&self, spec: &str, value: String) {
        let mut prompts = self.prompts.borrow_mut();
        match prompts.iter_mut().find(|(key, _)| key == spec) {
            Some(entry) => entry.1 = value,
            None => prompts.push((spec.to_string(), value)),
        }
    }
}

/// パスのプレースホルダー置換情報
pub struct PathPlaceholders {
    pub p: String,  // フルパス
    pub p_: String, // 拡張子なしパス ($-p)
    pub d: String,  // 親ディレクトリパス
    pub n: String,  // ファイル名/ディレクトリ名
    pub a: String,  // 拡張子なしファイル名
    pub f: String,  // 親ディレクトリ名
    pub e: String,  // 拡張子
}

impl PathPlaceholders {
    /// パスからプレースホルダー情報を作成
    pub fn from_path(path: &Path) -> Self {
        let path_str = path.to_string_lossy().to_string();
        let parent = path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let parent_name = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if path.is_dir() {
            PathPlaceholders {
                p: path_str.clone(),
                p_: path_str,
                d: parent,
                n: file_name.clone(),
                a: file_name,
                f: parent_name,
                e: String::new(),
            }
        } else {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let without_ext = if !parent.is_empty() && !stem.is_empty() {
                format!("{}\\{}", parent, stem)
            } else {
                stem.clone()
            };

            PathPlaceholders {
                p: path_str,
                p_: without_ext,
                d: parent,
                n: file_name,
                a: stem,
                f: parent_name,
                e: path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_string(),
            }
        }
    }

    /// `$` に続く記号に対応する置換値と、記号のバイト数を返す
    fn lookup(&self, rest: &[u8]) -> Option<(&str, usize)> {
        if rest.starts_with(b"-p") {
            return Some((&self.p_, 2));
        }

        let value = match rest.first()? {
            b'p' => &self.p,
            b'd' => &self.d,
            b'n' => &self.n,
            b'a' => &self.a,
            b'f' => &self.f,
            b'e' => &self.e,
            _ => return None,
        };
        Some((value, 1))
    }

    /// 文字列内のエスケープとプレースホルダーを置換
    pub fn replace(&self, text: &str, ctx: &RunContext) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len() + self.p.len());
        let mut chunk = 0;
        let mut i = 0;

        // `^` も `$` も ASCII なので、バイト位置は常に文字境界になる
        while i < bytes.len() {
            match bytes[i] {
                b'^' if i + 1 < bytes.len() && SPECIALS.contains(&bytes[i + 1]) => {
                    out.push_str(&text[chunk..i]);
                    out.push(bytes[i + 1] as char);
                    i += 2;
                    chunk = i;
                }
                // `$t{...}` は書式文字列を伴うので、1 文字の記号より先に見る。
                // 閉じていない場合はプレースホルダーとして扱わずそのまま残す
                // （パース時にエラーにしているので、通常はここに来ない）
                b'$' if bytes[i + 1..].starts_with(b"t{") => {
                    match bytes[i + 3..].iter().position(|b| *b == b'}') {
                        Some(end) => {
                            let spec = &text[i + 3..i + 3 + end];
                            out.push_str(&text[chunk..i]);
                            out.push_str(&ctx.now.format(spec));
                            i = i + 3 + end + 1;
                            chunk = i;
                        }
                        None => i += 1,
                    }
                }
                // `$?{...}` は起動より前に答えを集めてある。ここは引くだけ。
                // 書き方の解釈は prompt.rs に任せる（決まりの語や中の `$t{...}` を
                // 数え違えないよう、終端の探し方をひとつにしておく）
                b'$' if bytes.get(i + 1) == Some(&b'?') => {
                    match crate::prompt::parse_at(text, i) {
                        Some((prompt, end)) => {
                            out.push_str(&text[chunk..i]);
                            match ctx.prompt_value(prompt.source) {
                                Some(value) => out.push_str(&value),
                                // 答えを集めずに呼んだとき。消してしまうと
                                // 気づけないので、書かれたまま残す
                                None => out.push_str(prompt.source),
                            }
                            i = end;
                            chunk = i;
                        }
                        // 入力欄ではない（PowerShell の `$?` など）
                        None => i += 1,
                    }
                }
                b'$' => match self.lookup(&bytes[i + 1..]) {
                    Some((value, len)) => {
                        out.push_str(&text[chunk..i]);
                        out.push_str(value);
                        i += 1 + len;
                        chunk = i;
                    }
                    None => i += 1,
                },
                _ => i += 1,
            }
        }

        out.push_str(&text[chunk..]);
        out
    }

    /// 引数リスト内のプレースホルダーを置換
    pub fn replace_args(&self, args: &[String], ctx: &RunContext) -> Vec<String> {
        args.iter().map(|arg| self.replace(arg, ctx)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn placeholders() -> PathPlaceholders {
        // 実在しないパスなので is_dir() は false になり、ファイルとして扱われる
        PathPlaceholders::from_path(&PathBuf::from("C:\\folder\\file.txt"))
    }

    /// 時刻を固定した実行時コンテキスト（2026-08-15 土曜 14:03:05）
    fn ctx() -> RunContext {
        RunContext::for_test()
    }

    #[test]
    fn すべてのプレースホルダーを置換する() {
        let ph = placeholders();
        assert_eq!(ph.replace("$p", &ctx()), "C:\\folder\\file.txt");
        assert_eq!(ph.replace("$-p", &ctx()), "C:\\folder\\file");
        assert_eq!(ph.replace("$d", &ctx()), "C:\\folder");
        assert_eq!(ph.replace("$n", &ctx()), "file.txt");
        assert_eq!(ph.replace("$a", &ctx()), "file");
        assert_eq!(ph.replace("$f", &ctx()), "folder");
        assert_eq!(ph.replace("$e", &ctx()), "txt");
    }

    #[test]
    fn 拡張子なしパスを先に解釈する() {
        let ph = placeholders();
        assert_eq!(ph.replace("$-p.7z", &ctx()), "C:\\folder\\file.7z");
        assert_eq!(
            ph.replace("$-p_opt.webp", &ctx()),
            "C:\\folder\\file_opt.webp"
        );
    }

    #[test]
    fn エスケープしたドルは置換されない() {
        let ph = placeholders();
        assert_eq!(ph.replace("^$p", &ctx()), "$p");
        assert_eq!(ph.replace("^$-p", &ctx()), "$-p");
        assert_eq!(ph.replace("^$p $p", &ctx()), "$p C:\\folder\\file.txt");
    }

    #[test]
    fn エスケープした特殊文字は記号だけが残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("^@filelist.txt", &ctx()), "@filelist.txt");
        assert_eq!(ph.replace("^|", &ctx()), "|");
        assert_eq!(ph.replace("^^", &ctx()), "^");
        assert_eq!(ph.replace("^&", &ctx()), "&");
    }

    /// 素の `&` は PowerShell の呼び出し演算子なので、そのまま渡す
    /// （アクセスキーの記法が効くのは項目名だけ）
    #[test]
    fn 素のアンパサンドはそのまま渡る() {
        let ph = placeholders();
        assert_eq!(ph.replace("& 'C:\\a.exe'", &ctx()), "& 'C:\\a.exe'");
    }

    #[test]
    fn 何もエスケープしないキャレットはそのまま残る() {
        let ph = placeholders();
        assert_eq!(
            ph.replace("C:\\Foo^Bar\\app.exe", &ctx()),
            "C:\\Foo^Bar\\app.exe"
        );
        assert_eq!(ph.replace("末尾^", &ctx()), "末尾^");
    }

    #[test]
    fn 未知の記号のドルはそのまま残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("$x", &ctx()), "$x");
        assert_eq!(ph.replace("100$", &ctx()), "100$");
    }

    // -----------------------------------------------------------------
    // 日時（$t{...}）
    //
    // 書式そのものの網羅は datetime.rs にある。ここで見るのは、パスの
    // プレースホルダーやエスケープと同居させたときの振る舞い
    // -----------------------------------------------------------------

    #[test]
    fn 日時をパスと組み合わせられる() {
        let ph = placeholders();
        assert_eq!(
            ph.replace("$-p_$t{yyyyMMdd}.zip", &ctx()),
            "C:\\folder\\file_20260815.zip"
        );
        assert_eq!(
            ph.replace("$d\\backup-$t{yyyy-MM-dd}\\$n", &ctx()),
            "C:\\folder\\backup-2026-08-15\\file.txt"
        );
    }

    /// `^$` を先に解決してしまうと、残った `$t{` が日時として拾われてしまう
    #[test]
    fn エスケープした日時は置換されない() {
        let ph = placeholders();
        assert_eq!(ph.replace("^$t{yyyy}", &ctx()), "$t{yyyy}");
        assert_eq!(ph.replace("^$t{yyyy} $t{yyyy}", &ctx()), "$t{yyyy} 2026");
    }

    /// `$t` 単独は書式ではないので、既存の設定の意味を変えない
    #[test]
    fn 中括弧のない_t_はそのまま残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("$t", &ctx()), "$t");
        assert_eq!(ph.replace("100$t 円", &ctx()), "100$t 円");
    }

    /// 閉じ忘れはパース時にエラーにしているので、ここでは素通しでよい
    #[test]
    fn 閉じていない日時はそのまま残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("$t{yyyy", &ctx()), "$t{yyyy");
    }

    // -----------------------------------------------------------------
    // 入力欄（$?{...}）
    //
    // 答えを集めるのは menu.rs / preview.rs の仕事。ここで見るのは、
    // 集めた答えの引き方とエスケープとの兼ね合い
    // -----------------------------------------------------------------

    #[test]
    fn 集めた答えに置き換わる() {
        let ph = placeholders();
        let ctx = ctx();
        ctx.set_prompt("$?{長辺=1280}", "800".to_string());

        assert_eq!(
            ph.replace("-resize $?{長辺=1280} $p", &ctx),
            "-resize 800 C:\\folder\\file.txt"
        );
    }

    /// 同じ内容の `$?{...}` は同じ答えになる（聞かれるのは 1 回）
    #[test]
    fn 同じ入力欄は同じ答えになる() {
        let ph = placeholders();
        let ctx = ctx();
        ctx.set_prompt("$?{幅}", "640".to_string());

        assert_eq!(ph.replace("-w $?{幅} -h $?{幅}", &ctx), "-w 640 -h 640");
    }

    /// `^$` を先に解決してしまうと、残った `$?{` が入力欄として拾われてしまう
    #[test]
    fn エスケープした入力欄は置換されない() {
        let ph = placeholders();
        let ctx = ctx();
        ctx.set_prompt("$?{幅}", "640".to_string());

        assert_eq!(ph.replace("^$?{幅}", &ctx), "$?{幅}");
        assert_eq!(ph.replace("^$?{幅} $?{幅}", &ctx), "$?{幅} 640");
    }

    /// `$?` 単独は入力欄ではないので、既存の設定の意味を変えない
    #[test]
    fn 中括弧のない疑問符はそのまま残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("$?", &ctx()), "$?");
        assert_eq!(ph.replace("-o $?x", &ctx()), "-o $?x");
    }

    /// 答えを集めずに呼んだときは、消さずに書かれたまま残す
    #[test]
    fn 答えのない入力欄はそのまま残る() {
        let ph = placeholders();
        assert_eq!(
            ph.replace("-w $?{幅} $p", &ctx()),
            "-w $?{幅} C:\\folder\\file.txt"
        );
    }

    #[test]
    fn 日本語を含む文字列でも壊れない() {
        let ph = placeholders();
        assert_eq!(
            ph.replace("出力先は $d です", &ctx()),
            "出力先は C:\\folder です"
        );
    }
}
