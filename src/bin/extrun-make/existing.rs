/*!
今ある設定ファイルを読む（読むだけ）

**書き戻さない。** このツールは貼る文字列を作るもので、コメントや整形を
壊さないために既存のファイルには触らない。読むのは 3 つのためだけ。

1. **別名を挿し込めるようにする** — `@apps\7-Zip\7z.exe` と書けるようにする。
   長いパスを手で打たずに済むのが、「コマンドラインに明るくない」への答えの
   もう半分
2. **「対象の種類」に別名を並べる** — `@画像` を持っている人がそれを選べる
3. **プレビューを正しくする** — `[@画像]` を書き出したとき、別名の定義が
   無いと `config::parse` は「未定義の別名」で止まる。設定ファイルの全文と
   繋げて解析すれば解決できる

**読めなくても動く。** 設定ファイルがまだ無い人・別名を 1 つも書いていない人
でも、ひな型とプレースホルダーはそのまま使える。読めなかったことが
このツールを止める理由にはならない。
*/

use std::path::PathBuf;

use extrun::config::{self, MenuItem};

/// 別名 1 つぶん
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub name: String,
    /// 書かれたままの値（他の別名を含むことがある）
    pub value: String,
}

/// 今あるサブメニュー 1 つぶん
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submenu {
    /// 「圧縮」や「圧縮 > ZIP」
    pub label: String,
    /// 何階層目か（1 なら `>`、2 なら `>>`）
    pub depth: usize,
    /// **この行の下に貼る**（いちばん最後の子の行）
    pub last_line: u32,
}

/// 今あるセクション 1 つぶん
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// 見出しの中身（`.png .jpg` や `@画像`）
    pub spec: String,
    /// 見出しの行
    pub line: u32,
    /// **この行の下に貼る**（このセクションの最後の中身の行）
    pub end_line: u32,
}

/// 読み込んだ設定ファイル
pub struct Existing {
    /// 読めたファイルの場所（読めなければ `None`）
    pub path: Option<PathBuf>,
    /// 書かれた順の別名
    pub aliases: Vec<Alias>,
    /// 今あるサブメニュー（2 階層目まで）
    pub submenus: Vec<Submenu>,
    /// 今あるセクション見出し（書かれた順）
    pub sections: Vec<Section>,
    /// プレビューで前に繋げる本文
    ///
    /// **書式が壊れているファイルは繋げない。** 繋げると、このツールで
    /// 作った行とは関係のないエラーがプレビューを埋めてしまう。
    pub prefix: String,
    /// 読めたが書式が壊れているときの理由
    pub problem: Option<String>,
}

impl Existing {
    /// `extrun.exe` と同じフォルダの設定ファイルを読む
    ///
    /// `extrun-make.exe` は `extrun.exe` の隣に置かれる前提なので、
    /// 探す場所も同じ（カレントディレクトリではない）。
    pub fn load() -> Existing {
        let Some(path) = default_path() else {
            return Existing::empty();
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return Existing::empty();
        };
        let Some(text) = decode(&bytes) else {
            return Existing::empty();
        };

        let aliases = collect_aliases(&text);
        let parsed = config::parse(&text);
        let submenus = if parsed.has_error() {
            // 壊れているファイルから読んだ階層は当てにならない
            Vec::new()
        } else {
            collect_submenus(&parsed.config.apps)
        };
        let sections = if parsed.has_error() {
            Vec::new()
        } else {
            collect_sections(&text)
        };
        let problem = if parsed.has_error() {
            Some(format!(
                "設定ファイルに {} 件の問題があります（extrun.exe --check で確認できます）",
                parsed.errors().count()
            ))
        } else {
            None
        };

        Existing {
            // 壊れているファイルは繋げない
            prefix: if problem.is_none() {
                text
            } else {
                String::new()
            },
            path: Some(path),
            aliases,
            submenus,
            sections,
            problem,
        }
    }

    fn empty() -> Existing {
        Existing {
            path: None,
            aliases: Vec::new(),
            submenus: Vec::new(),
            sections: Vec::new(),
            prefix: String::new(),
            problem: None,
        }
    }

    /// 「対象の種類」に並べられる別名（値が拡張子の並びに見えるもの）
    ///
    /// レシピ集を全部写した人で別名は 34 種類あるが、拡張子の並びはそのうち
    /// 5 種類（`@画像` `@音声` `@動画` `@書庫` `@テキスト`）。
    /// パスの別名を混ぜると一覧が読めなくなる。
    pub fn extension_aliases(&self) -> Vec<&Alias> {
        self.aliases
            .iter()
            .filter(|alias| looks_like_extensions(&alias.value))
            .collect()
    }

    /// 画面に出す状態の 1 行
    pub fn status(&self) -> String {
        if let Some(problem) = &self.problem {
            return problem.clone();
        }
        match &self.path {
            None => "設定ファイルが見つかりません（別名は使えません）".to_string(),
            Some(_) if self.aliases.is_empty() => "設定ファイルに別名はありません".to_string(),
            Some(_) => format!("設定ファイルから別名を {} 件読みました", self.aliases.len()),
        }
    }
}

/// 今あるセクション見出しを、書かれた順に集める
///
/// **見分け方は `config::as_section` に任せる。** 自前で `[` と `]` を見ると、
/// 片方だけが `[a] b` のような行を通す事故になる。
///
/// 貼り先は**見出しの行ではなく、そのセクションの最後の中身の行**。
/// 見出しの直後に入れると、既にある項目より前に割り込む。空行を跨がないよう、
/// 中身のある最後の行まで下げる。
fn collect_sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found: Vec<Section> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(spec) = config::as_section(trimmed) else {
            continue;
        };
        // `[extrun]` は拡張子のセクションではない
        if spec.eq_ignore_ascii_case("extrun") {
            continue;
        }

        let line_number = index as u32 + 1;
        if let Some(previous) = found.last_mut() {
            previous.end_line = last_content_line(&lines, previous.line, line_number - 1);
        }
        found.push(Section {
            spec: spec.trim().to_string(),
            line: line_number,
            end_line: line_number,
        });
    }

    if let Some(last) = found.last_mut() {
        last.end_line = last_content_line(&lines, last.line, lines.len() as u32);
    }
    found
}

/// `from`〜`to` の範囲で、中身のある最後の行
fn last_content_line(lines: &[&str], from: u32, to: u32) -> u32 {
    let mut result = from;
    for number in from..=to {
        if let Some(line) = lines.get(number as usize - 1) {
            if !line.trim().is_empty() {
                result = number;
            }
        }
    }
    result
}

/// 今あるサブメニューを、書かれた順に集める
///
/// **2 階層目まで。** 3 階層目に足すのは `>` を 1 つ書き足せば済むし、
/// そこまで並べると一覧が読めなくなる（`extrun-make` の役目は最初の 1 歩を
/// 楽にすることで、メニュー全体の編集ではない）。
fn collect_submenus(items: &[MenuItem]) -> Vec<Submenu> {
    let mut found = Vec::new();
    walk_submenus(items, &mut Vec::new(), &mut found);
    found
}

fn walk_submenus(items: &[MenuItem], path: &mut Vec<String>, found: &mut Vec<Submenu>) {
    for item in items {
        if !item.has_submenu() {
            continue;
        }
        path.push(item.name.clone());

        found.push(Submenu {
            label: path.join(" > "),
            depth: path.len(),
            last_line: last_line(item),
        });

        if path.len() < 2 {
            walk_submenus(&item.submenu, path, found);
        }
        path.pop();
    }
}

/// その項目とその中身のうち、いちばん下の行
///
/// **子の下に貼る**ので、親の行ではなく末端まで見る。ここを親の行にすると、
/// 既にある子より前に割り込んでしまう。
fn last_line(item: &MenuItem) -> u32 {
    item.submenu
        .iter()
        .map(last_line)
        .chain(std::iter::once(item.line))
        .max()
        .unwrap_or(item.line)
}

/// `extrun.exe` と同じフォルダの `extrun-config.txt`
fn default_path() -> Option<PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .parent()?
            .join(config::CONFIG_FILE_NAME),
    )
}

/// BOM を読み飛ばして UTF-8 として解釈する
fn decode(bytes: &[u8]) -> Option<String> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(body.to_vec()).ok()
}

/// `@名前 = 値` の行を書かれた順に集める
///
/// **見分け方は `config::as_alias_def` に任せる。** 自前で `=` を探すと、
/// `@a b = c` のような書き方を片方だけが通す事故になる。
fn collect_aliases(text: &str) -> Vec<Alias> {
    let mut aliases = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        // コメントは行ごと読み飛ばす
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = config::as_alias_def(trimmed) {
            aliases.push(Alias {
                name: name.to_string(),
                value: value.to_string(),
            });
        }
    }

    aliases
}

/// 値が拡張子の並びに見えるか
///
/// 空白で区切った要素がすべて `.` で始まるか、`file` / `folder` のとき。
/// **大文字小文字は区別しない**（設定ファイル側もそうなっている）。
fn looks_like_extensions(value: &str) -> bool {
    let mut any = false;
    for token in value.split_whitespace() {
        any = true;
        let lower = token.to_lowercase();
        if !lower.starts_with('.') && lower != "file" && lower != "folder" {
            return false;
        }
    }
    any
}

/// 長い値を後ろ側だけ残して詰める（パス向け）
///
/// パスは**末尾のファイル名が識別に効く**ので、前を削る。
pub fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let skip = count - max + 1;
    format!("…{}", text.chars().skip(skip).collect::<String>())
}

/// 長い値を前側だけ残して詰める（拡張子の並び向け）
///
/// 拡張子の並びは**先頭のいくつかで見当が付く**ので、後ろを削る。
pub fn head(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    format!("{}…", text.chars().take(max - 1).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 別名の定義を書かれた順に集める() {
        let 本文 =
            "@win = %SystemRoot%\r\n@sys = @win\\System32\r\n\r\n[.txt]\r\nX | @sys\\a.exe\r\n";
        let 別名 = collect_aliases(本文);
        assert_eq!(別名.len(), 2);
        assert_eq!(別名[0].name, "win");
        assert_eq!(別名[1].value, "@win\\System32");
    }

    #[test]
    fn コメントの中の別名は拾わない() {
        assert!(collect_aliases("# @win = C:\\\r\n").is_empty());
    }

    /// 見分け方はパーサと同じ（`@a b = c` は別名ではない）
    #[test]
    fn 名前に空白があれば別名ではない() {
        assert!(collect_aliases("@a b = c\r\n").is_empty());
    }

    #[test]
    fn 拡張子の並びだけを選び出す() {
        let existing = Existing {
            path: None,
            prefix: String::new(),
            problem: None,
            submenus: Vec::new(),
            sections: Vec::new(),
            aliases: vec![
                Alias {
                    name: "画像".into(),
                    value: ".png .jpg".into(),
                },
                Alias {
                    name: "apps".into(),
                    value: "C:\\Program Files".into(),
                },
                Alias {
                    name: "全部".into(),
                    value: "file folder".into(),
                },
                Alias {
                    name: "大文字".into(),
                    value: "FILE".into(),
                },
            ],
        };
        let 拡張子 = existing.extension_aliases();
        assert_eq!(拡張子.len(), 3);
        assert_eq!(拡張子[0].name, "画像");
        assert_eq!(拡張子[1].name, "全部");
        assert_eq!(拡張子[2].name, "大文字");
    }

    #[test]
    fn 空の値は拡張子の並びではない() {
        assert!(!looks_like_extensions(""));
        assert!(!looks_like_extensions("   "));
    }

    /// パスは末尾のファイル名が識別に効く
    #[test]
    fn パスは前を削る() {
        let 詰めた = tail("C:\\Program Files\\Microsoft VS Code\\Code.exe", 20);
        assert_eq!(詰めた.chars().count(), 20);
        assert!(詰めた.starts_with('…'), "{}", 詰めた);
        assert!(詰めた.ends_with("\\Code.exe"), "{}", 詰めた);

        assert_eq!(tail("短い", 20), "短い");
    }

    /// 拡張子の並びは先頭で見当が付く
    #[test]
    fn 拡張子の並びは後ろを削る() {
        assert_eq!(head(".png .jpg .jpeg .gif .bmp", 12), ".png .jpg .…");
        assert_eq!(head(".png", 12), ".png");
    }

    fn 階層(本文: &str) -> Vec<Submenu> {
        let parsed = config::parse(本文);
        assert!(!parsed.has_error());
        collect_submenus(&parsed.config.apps)
    }

    #[test]
    fn 今あるサブメニューを集める() {
        let 本文 = "[file]\r\n\
            圧縮\r\n\
            > ZIP\r\n\
            >> 個別に | C:\\a.exe\r\n\
            >> まとめて | C:\\a.exe\r\n\
            > TAR\r\n\
            >> 個別に | C:\\a.exe\r\n\
            開く | C:\\b.exe\r\n";
        let 階層 = 階層(本文);

        assert_eq!(
            階層.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
            ["圧縮", "圧縮 > ZIP", "圧縮 > TAR"]
        );
        assert_eq!(階層[0].depth, 1);
        assert_eq!(階層[1].depth, 2);
    }

    /// 親の行ではなく末端の行を返す（既にある子より前に割り込まないため）
    #[test]
    fn 貼る先はいちばん最後の子の下() {
        let 本文 = "[file]\r\n\
            圧縮\r\n\
            > ZIP\r\n\
            >> 個別に | C:\\a.exe\r\n\
            >> まとめて | C:\\a.exe\r\n";
        let 階層 = 階層(本文);

        // 「圧縮」の末端も「圧縮 > ZIP」の末端も、いちばん下の子の行
        assert_eq!(階層[0].last_line, 5);
        assert_eq!(階層[1].last_line, 5);
    }

    /// 3 階層目は並べない（`>` を 1 つ書き足せば済む）
    #[test]
    fn 三階層目は集めない() {
        let 本文 = "[file]\r\n\
            A\r\n\
            > B\r\n\
            >> C\r\n\
            >>> D | C:\\a.exe\r\n";
        let 階層 = 階層(本文);
        assert_eq!(
            階層.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
            ["A", "A > B"]
        );
    }

    #[test]
    fn 今あるセクションを集める() {
        let 本文 = "@a = b\r\n\
            \r\n\
            [.png .jpg]\r\n\
            X | C:\\a.exe\r\n\
            Y | C:\\b.exe\r\n\
            \r\n\
            [folder]\r\n\
            Z | C:\\c.exe\r\n\
            \r\n";
        let 節 = collect_sections(本文);

        assert_eq!(節.len(), 2);
        assert_eq!(節[0].spec, ".png .jpg");
        assert_eq!(節[0].line, 3);
        // 空行を跨がず、中身のある最後の行まで
        assert_eq!(節[0].end_line, 5);
        assert_eq!(節[1].spec, "folder");
        assert_eq!(節[1].end_line, 8);
    }

    /// `[extrun]` は拡張子のセクションではない
    #[test]
    fn グローバル設定は集めない() {
        let 本文 = "[extrun]\r\nicons = auto\r\n\r\n[file]\r\nX | C:\\a.exe\r\n";
        let 節 = collect_sections(本文);
        assert_eq!(節.len(), 1);
        assert_eq!(節[0].spec, "file");
    }

    #[test]
    fn コメントの中の見出しは拾わない() {
        assert!(collect_sections("# [.png]\r\n").is_empty());
    }

    #[test]
    fn 読めなければ何も無い状態になる() {
        let empty = Existing::empty();
        assert!(empty.aliases.is_empty());
        assert!(empty.submenus.is_empty());
        assert!(empty.sections.is_empty());
        assert!(empty.prefix.is_empty());
        assert!(empty.status().contains("見つかりません"));
    }
}
