/*!
ライブプレビュー（⑤ この設定で起動されるもの）

**このツールの目玉。** 友人の「設定したあとの動きが想像できない」への
直接の答えで、入力するたびに「その設定で実際に起動されるコマンドライン」を
出す。

**組み立ては ExtRun 本体と同じ道を通る。**

    フォーム
      → 作成した設定（④ の欄）        ← ここが中間表現を兼ねる
      → config::parse                  → 書式のエラーをその場で出す
      → 末尾の項目を取り出す
      → filter::filter_menu_items      → この対象で表示されるか
      → preview::write_invocations     → ⑤ の欄
                                          （中で invoke::resolve_invocations）

④ を中間表現にしてあるので、ツールの中に「フォームの状態」と「設定の文字列」
という 2 つの真実が並ばない。**貼る文字列がそのまま検証にもプレビューにも
使われる。**

整形は `--preview` と同じ関数（`preview::write_invocations`）を呼ぶ。
自前で書くと、プレビューが `--preview` と違うことを言い出す。
*/

use extrun::config::{self, MenuItem};
use extrun::placeholder::RunContext;
use extrun::{Target, filter, preview};
use std::path::{Path, PathBuf};

/// 試す対象の既定値
///
/// 仕様書がプレースホルダーの説明に使っている例と同じにしてある
/// （読み比べたときに `$d` が何を指すのかがすぐ分かる）。
pub const DEFAULT_TARGET: &str = r"C:\folder\file.txt";

/// 何個選んだことにするか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    One,
    Three,
}

impl Count {
    fn len(self) -> usize {
        match self {
            Count::One => 1,
            Count::Three => 3,
        }
    }
}

/// 試す対象を組み立てる
///
/// 3 つのときは、打たれたパスと**同じフォルダ・同じ拡張子の仲間**を足す。
/// 別の拡張子を混ぜると絞り込みの規則（積集合）が働いて項目が消え、
/// 「なぜ出ないのか」を考える対象がずれる。
pub fn targets(path: &str, count: Count) -> Vec<Target> {
    let path = path.trim();
    if path.is_empty() {
        return Vec::new();
    }

    let base = PathBuf::from(path);
    let mut targets = vec![Target::from_path(base.clone())];

    for index in 2..=count.len() {
        targets.push(Target::from_path(sibling(&base, index)));
    }
    targets
}

/// 同じフォルダに `名前-2.拡張子` のような仲間を作る
fn sibling(base: &Path, index: usize) -> PathBuf {
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = base
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();

    let name = format!("{}-{}{}", stem, index, extension);
    match base.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// 「この設定で起動されるもの」に出す本文を組み立てる
///
/// `config_text` は「作成した設定」そのもの。`--check` と同じ検証を通るので、
/// 書式が壊れているあいだは**その理由がここに出る**。
///
/// `prefix` は今ある設定ファイルの本文。**前に繋げてから解析する**のは、
/// `[@画像]` や `@sys\tar.exe` のような別名が定義を必要とするため。
/// 繋げないと「未定義の別名」で止まり、別名を使った設定のプレビューが
/// 何も出せなくなる。行番号がずれるので、**診断はこのツールが作った行に
/// 限って出す**。
pub fn describe(
    prefix: &str,
    after_line: Option<u32>,
    config_text: &str,
    path: &str,
    count: Count,
) -> String {
    let (joined, offset) = splice(prefix, after_line, config_text);
    let parsed = config::parse(&joined);

    if parsed.has_error() {
        let mut out = String::from("設定の書き方に問題があります。\r\n\r\n");
        for diag in parsed.errors() {
            // 前に繋げたぶんを引いて、作成した設定の中での行番号にする
            let line = diag.line.saturating_sub(offset).max(1);
            out.push_str(&format!("{} 行目  {}\r\n", line, diag.message));
        }
        return out;
    }

    let targets = targets(path, count);
    if targets.is_empty() {
        return "試す対象のパスを入れてください。\r\n".to_string();
    }

    // **差し込んだ行から探す。** 途中に入れたときは、ツールが作った項目が
    // 末尾にいるとは限らない
    let made = generated_lines(config_text, offset);
    let Some(item) = made
        .clone()
        .and_then(|range| item_at(&parsed.config.apps, range))
        .or_else(|| last_item(&parsed.config.apps).cloned())
    else {
        return "まだ項目がありません。\r\n".to_string();
    };
    let item = &item;

    // 対象に合うかどうかは、メニューを組み立てるときと同じ関数で見る。
    // ここで自前に判定すると「プレビューには出るのにメニューに出ない」が起きる
    let shown = filter::filter_menu_items(std::slice::from_ref(item), &targets);
    if shown.iter().all(|item| item.is_separator()) {
        let mut out = String::from("この対象ではメニューに表示されません。\r\n\r\n");
        out.push_str(&filter::empty_menu_message(&targets).replace('\n', "\r\n"));
        out.push_str("\r\n");
        return out;
    }

    let mut out = String::new();
    out.push_str("対象:\r\n");
    for target in &targets {
        out.push_str(&format!(
            "  {}  ({})\r\n",
            target.path.display(),
            target.file_type
        ));
    }
    out.push_str("\r\n");

    // 実行するときと同じように、ここで 1 回だけ確定させる
    let ctx = RunContext::capture(targets.len());
    preview::write_invocations(item, &targets, &ctx, &parsed.config, &mut out);

    out
}

/// 今ある設定ファイルの、**実際に貼る場所**へ差し込む
///
/// 末尾に繋げるだけでは足りない。`>` の付いた行を末尾に足すと、選んだ親では
/// なく**ファイル最後のルート項目**にぶら下がるので、プレビューが嘘になる。
/// 拡張子の差分（`+` / `-`）も、どのセクションの下にいるかで結果が変わる。
///
/// 戻り値の 2 つ目は、差し込んだ位置より前にある行数。**エラーの行番号から
/// これを引く**と、「作成した設定」の中での行番号になる。
fn splice(prefix: &str, after_line: Option<u32>, config_text: &str) -> (String, u32) {
    if prefix.trim().is_empty() {
        return (config_text.to_string(), 0);
    }

    let lines: Vec<&str> = prefix.lines().collect();
    let at = match after_line {
        // 1 始まりの行番号。その行の**下**に入れる
        Some(line) => (line as usize).min(lines.len()),
        None => lines.len(),
    };

    let mut out = String::new();
    for line in &lines[..at] {
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(config_text);
    for line in &lines[at..] {
        out.push_str(line);
        out.push_str("\r\n");
    }

    (out, at as u32)
}

/// いちばん最後の（＝ツールが作った）項目を取り出す
///
/// サブメニューを作った場合、親ではなく**中身**が見たいものなので、
/// 末尾までたどる。
fn last_item(items: &[MenuItem]) -> Option<&MenuItem> {
    let last = items.iter().rev().find(|item| !item.is_separator())?;
    if last.has_submenu() {
        last_item(&last.submenu)
    } else {
        Some(last)
    }
}

/// ツールが作った行が、繋げた本文の何行目から何行目までにあるか
fn generated_lines(config_text: &str, offset: u32) -> Option<std::ops::Range<u32>> {
    let count = config_text.lines().count() as u32;
    if count == 0 {
        return None;
    }
    Some(offset + 1..offset + 1 + count)
}

/// その行の範囲にある項目のうち、いちばん下のもの
///
/// サブメニューの中まで探す（親ではなく中身が見たいものなので）。
fn item_at(items: &[MenuItem], range: std::ops::Range<u32>) -> Option<MenuItem> {
    let mut found: Option<MenuItem> = None;

    for item in items {
        if let Some(deeper) = item_at(&item.submenu, range.clone()) {
            found = Some(deeper);
            continue;
        }
        if !item.is_separator() && !item.has_submenu() && range.contains(&item.line) {
            found = Some(item.clone());
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const 見本: &str = "[.png]\r\n\r\n+ ZIP にまとめる | C:\\tar.exe | -c -f $d\\a.zip $p\r\n";

    /// 設定ファイルを読んでいないときの呼び方
    fn describe(config_text: &str, path: &str, count: Count) -> String {
        super::describe("", None, config_text, path, count)
    }

    /// 設定ファイルを前に繋げるときの呼び方
    fn describe_with(prefix: &str, config_text: &str) -> String {
        super::describe(prefix, None, config_text, r"C:\photo\a.png", Count::One)
    }

    #[test]
    fn 一つのときは打たれたパスだけ() {
        let targets = targets(r"C:\photo\a.png", Count::One);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].file_type, ".png");
    }

    /// 別の拡張子を混ぜると絞り込みが働いて項目が消えてしまう
    #[test]
    fn 三つのときは同じ拡張子の仲間を足す() {
        let targets = targets(r"C:\photo\a.png", Count::Three);
        assert_eq!(targets.len(), 3);
        assert!(targets.iter().all(|t| t.file_type == ".png"));
        assert_eq!(targets[1].path, PathBuf::from(r"C:\photo\a-2.png"));
        assert_eq!(targets[2].path, PathBuf::from(r"C:\photo\a-3.png"));
    }

    #[test]
    fn 拡張子が無くても仲間を作れる() {
        let targets = targets(r"C:\photo\README", Count::Three);
        assert_eq!(targets[1].path, PathBuf::from(r"C:\photo\README-2"));
    }

    #[test]
    fn 起動されるものが出る() {
        let 本文 = describe(見本, r"C:\photo\a.png", Count::One);
        assert!(本文.contains("実行ファイル"), "{}", 本文);
        assert!(本文.contains(r"C:\tar.exe"), "{}", 本文);
        assert!(本文.contains(r"C:\photo\a.zip"), "{}", 本文);
    }

    /// `+` は 1 プロセスにまとまるので、3 つ選んでも起動は 1 回
    #[test]
    fn まとめて渡すと三つ選んでも一回() {
        let 本文 = describe(見本, r"C:\photo\a.png", Count::Three);
        assert!(本文.contains(r"C:\photo\a-2.png"), "{}", 本文);
        assert!(!本文.contains("[1/3]"), "{}", 本文);
    }

    /// `+` を外すと対象の数だけ起動する
    #[test]
    fn まとめないと対象の数だけ起動する() {
        let 設定 = 見本.replace("+ ZIP", "ZIP");
        let 本文 = describe(&設定, r"C:\photo\a.png", Count::Three);
        assert!(本文.contains("[1/3]"), "{}", 本文);
        assert!(本文.contains("[3/3]"), "{}", 本文);
    }

    #[test]
    fn 対象に合わなければ表示されないと言う() {
        let 本文 = describe(見本, r"C:\doc\a.txt", Count::One);
        assert!(本文.contains("表示されません"), "{}", 本文);
    }

    /// 書式が壊れているあいだは `--check` と同じ理由を出す
    #[test]
    fn 書式のエラーをその場で出す() {
        let 本文 = describe(
            "[.png]\r\n\r\nX | C:\\a.exe\r\n :when いつも\r\n",
            r"C:\a.png",
            Count::One,
        );
        assert!(本文.contains("問題があります"), "{}", 本文);
        assert!(本文.contains("行目"), "{}", 本文);
    }

    #[test]
    fn 項目が無ければそう言う() {
        assert!(describe("[.png]\r\n", r"C:\a.png", Count::One).contains("まだ項目がありません"));
    }

    #[test]
    fn 対象が空ならそう言う() {
        assert!(describe(見本, "", Count::One).contains("パスを入れてください"));
    }

    /// 別名は設定ファイルの側にしか定義が無いので、繋げないと解決できない
    #[test]
    fn 設定ファイルの別名を解決する() {
        let 設定 = "[.png]\r\n\r\nX | @sys\\tar.exe | $p\r\n";
        let 本文 = describe_with("@sys = C:\\Windows\\System32\r\n", 設定);
        assert!(本文.contains(r"C:\Windows\System32\tar.exe"), "{}", 本文);
    }

    /// 前に繋げたぶんの行数を引かないと、エラーの行番号が読めない
    #[test]
    fn エラーの行番号は作成した設定の中で数える() {
        let 設定 = "[.png]\r\n\r\nX | C:\\a.exe | $t{zzz}\r\n";
        let 単体 = super::describe("", None, 設定, r"C:\a.png", Count::One);
        let 連結 = super::describe(
            "@a = b\r\n@c = d\r\n@e = f\r\n",
            None,
            設定,
            r"C:\a.png",
            Count::One,
        );
        let 行 = |text: &str| {
            text.lines()
                .find(|line| line.contains("行目"))
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(行(&単体), 行(&連結), "行番号がずれている");
    }

    /// 末尾に繋げると、`>` の行がファイル最後のルート項目にぶら下がる
    #[test]
    fn 今あるサブメニューの中に差し込む() {
        let 元 = "[file]\r\n\
            圧縮\r\n\
            > ZIP | C:\\zip.exe | $p\r\n\
            別のもの\r\n\
            > X | C:\\x.exe | $p\r\n";
        // 3 行目（`> ZIP` の行）の下に差し込む
        let 本文 = super::describe(
            元,
            Some(3),
            "> 新しい項目 | C:\\new.exe | $p\r\n",
            r"C:\a.txt",
            Count::One,
        );
        assert!(本文.contains(r"C:\new.exe"), "{}", 本文);
    }

    /// 差し込んだ位置より前の行数を引かないと、行番号が読めない
    #[test]
    fn 途中に差し込んでも行番号は作成した設定の中で数える() {
        let 設定 = "[.png]\r\n\r\nX | C:\\a.exe | $t{zzz}\r\n";
        let 単体 = super::describe("", None, 設定, r"C:\a.png", Count::One);
        let 途中 = super::describe(
            "[file]\r\nA | C:\\a.exe\r\nB | C:\\b.exe\r\n",
            Some(2),
            設定,
            r"C:\a.png",
            Count::One,
        );
        let 行 = |text: &str| {
            text.lines()
                .find(|line| line.contains("行目"))
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(行(&単体), 行(&途中), "行番号がずれている");
    }

    /// サブメニューを作ったときは、親ではなく中身を見せる
    #[test]
    fn サブメニューの中身を取り出す() {
        let 設定 = "[.png]\r\n\r\n圧縮\r\n> ZIP にまとめる | C:\\tar.exe | $p\r\n";
        let 本文 = describe(設定, r"C:\photo\a.png", Count::One);
        assert!(本文.contains(r"C:\tar.exe"), "{}", 本文);
    }
}
