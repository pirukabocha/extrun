/*!
プレースホルダーの置換処理

引数と作業フォルダはパース時に `^` を残したまま渡ってくる。ここで左から 1 回
走査して `^X` → `X` と `$x` → 置換値 を同時に処理する。先にエスケープだけを
解決すると `^$` が `$` になったあとプレースホルダーとして拾われてしまう。
*/

use crate::config::SPECIALS;
use std::path::Path;

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
    pub fn replace(&self, text: &str) -> String {
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
    pub fn replace_args(&self, args: &[String]) -> Vec<String> {
        args.iter().map(|arg| self.replace(arg)).collect()
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

    #[test]
    fn すべてのプレースホルダーを置換する() {
        let ph = placeholders();
        assert_eq!(ph.replace("$p"), "C:\\folder\\file.txt");
        assert_eq!(ph.replace("$-p"), "C:\\folder\\file");
        assert_eq!(ph.replace("$d"), "C:\\folder");
        assert_eq!(ph.replace("$n"), "file.txt");
        assert_eq!(ph.replace("$a"), "file");
        assert_eq!(ph.replace("$f"), "folder");
        assert_eq!(ph.replace("$e"), "txt");
    }

    #[test]
    fn 拡張子なしパスを先に解釈する() {
        let ph = placeholders();
        assert_eq!(ph.replace("$-p.7z"), "C:\\folder\\file.7z");
        assert_eq!(ph.replace("$-p_opt.webp"), "C:\\folder\\file_opt.webp");
    }

    #[test]
    fn エスケープしたドルは置換されない() {
        let ph = placeholders();
        assert_eq!(ph.replace("^$p"), "$p");
        assert_eq!(ph.replace("^$-p"), "$-p");
        assert_eq!(ph.replace("^$p $p"), "$p C:\\folder\\file.txt");
    }

    #[test]
    fn エスケープした特殊文字は記号だけが残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("^@filelist.txt"), "@filelist.txt");
        assert_eq!(ph.replace("^|"), "|");
        assert_eq!(ph.replace("^^"), "^");
        assert_eq!(ph.replace("^&"), "&");
    }

    /// 素の `&` は PowerShell の呼び出し演算子なので、そのまま渡す
    /// （アクセスキーの記法が効くのは項目名だけ）
    #[test]
    fn 素のアンパサンドはそのまま渡る() {
        let ph = placeholders();
        assert_eq!(ph.replace("& 'C:\\a.exe'"), "& 'C:\\a.exe'");
    }

    #[test]
    fn 何もエスケープしないキャレットはそのまま残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("C:\\Foo^Bar\\app.exe"), "C:\\Foo^Bar\\app.exe");
        assert_eq!(ph.replace("末尾^"), "末尾^");
    }

    #[test]
    fn 未知の記号のドルはそのまま残る() {
        let ph = placeholders();
        assert_eq!(ph.replace("$x"), "$x");
        assert_eq!(ph.replace("100$"), "100$");
    }

    #[test]
    fn 日本語を含む文字列でも壊れない() {
        let ph = placeholders();
        assert_eq!(ph.replace("出力先は $d です"), "出力先は C:\\folder です");
    }
}
