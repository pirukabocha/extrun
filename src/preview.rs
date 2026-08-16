/*!
`extrun.exe --preview` の実装

対象に対して表示されるメニュー項目と、そこから実際に起動されるコマンドラインを、
起動せずにコンソールへ書き出す。引数のエスケープとプレースホルダーが意図どおりに
解決されているかを、プロセスを走らせずに確かめるための出力。

コマンドラインの組み立ては `menu::resolve_invocations` に任せる。ここで組み立て
直すと、表示しているものと実際に起動されるものが静かにずれていく。
*/

use crate::config::{Config, MenuItem};
use crate::console;
use crate::menu::{filter_menu_items, resolve_invocations};
use crate::placeholder::{PathPlaceholders, RunContext};
use crate::Target;
use std::path::Path;

/// 対象に対するメニューの内容を出力し、終了コードを返す
///
/// 設定ファイルが読めない、またはエラーを含む場合は 1。`--check` と違い警告は
/// 数えない（表示はできるので、実際に何が起動されるかを見せる方が役に立つ）。
pub fn run(config_path: &Path, targets: &[Target]) -> i32 {
    let parsed = match Config::load(config_path) {
        Ok(parsed) => parsed,
        Err(message) => {
            console::print(&format!("{}\r\n", message.replace('\n', "\r\n")));
            return 1;
        }
    };

    if parsed.has_error() {
        let mut out = String::from("設定ファイルにエラーがあるため表示できません。\r\n\r\n");
        for diag in parsed.errors() {
            out.push_str(&format!("{:>4}行目  {}\r\n", diag.line, diag.message));
        }
        out.push_str("\r\n詳しくは extrun.exe --check で確認できます。\r\n");
        console::print(&out);
        return 1;
    }

    // 実行するときと同じように、ここで 1 回だけ確定させる。表示される日時は
    // プレビューを実行した時刻になる
    console::print(&report(&parsed.config, targets, &RunContext::capture()));
    0
}

/// 出力する本文を組み立てる
fn report(config: &Config, targets: &[Target], ctx: &RunContext) -> String {
    let mut out = String::from("対象:\r\n");
    for target in targets {
        out.push_str(&format!(
            "  {}  ({})\r\n",
            target.path.display(),
            target.file_type
        ));
    }

    let items = filter_menu_items(&config.apps, targets);
    if items.is_empty() {
        out.push_str("\r\n対象となるファイルに適用できるメニュー項目がありません。\r\n");
        return out;
    }

    let mut count = 0;
    write_items(
        &items,
        &mut Vec::new(),
        targets,
        ctx,
        config,
        &mut out,
        &mut count,
    );
    out.push_str(&format!("\r\n{} 項目\r\n", count));
    out
}

/// メニューを辿って、実行できる項目ごとの内容を書き出す
///
/// サブメニューの親は選んでも実行されないので、`親 > 子` の形で名前だけ引き継ぐ。
/// セパレーターも実行の対象ではないので飛ばす。
#[allow(clippy::too_many_arguments)]
fn write_items(
    items: &[MenuItem],
    parents: &mut Vec<String>,
    targets: &[Target],
    ctx: &RunContext,
    config: &Config,
    out: &mut String,
    count: &mut usize,
) {
    for item in items {
        if item.is_separator() {
            continue;
        }

        if item.has_submenu() {
            parents.push(item.name.clone());
            write_items(&item.submenu, parents, targets, ctx, config, out, count);
            parents.pop();
            continue;
        }

        *count += 1;
        out.push_str("\r\n");
        for parent in parents.iter() {
            out.push_str(parent);
            out.push_str(" > ");
        }
        out.push_str(&item.name);
        out.push_str("\r\n");

        write_invocations(item, targets, ctx, config, out);
    }
}

/// 行の見出し
///
/// 語は設定ファイルの仕様と `--check` のメッセージに合わせる（`作業` のように
/// 縮めると何を指しているのか読み取れない）。幅は全角スペースで揃える。
/// `check.rs` の `警告　` と同じやり方で、等幅フォントなら桁が合う。
const LABEL_PROMPT: &str = "入力　　　　";
const LABEL_CONFIRM: &str = "実行前の確認";
const LABEL_PROGRAM: &str = "実行ファイル";
const LABEL_ARG: &str = "引数　　　　";
const LABEL_DIR: &str = "作業フォルダ";
const LABEL_ADMIN: &str = "実行の権限　";
const LABEL_DELAY: &str = "起動の間隔　";

/// 1 項目から起動されるプロセスを書き出す
///
/// 引数は 1 つ 1 行にする。空白で連結すると、PowerShell に渡す長い 1 引数と
/// 複数の引数の区別がつかなくなる（引数の切れ目こそ確かめたいもの）。
fn write_invocations(
    item: &MenuItem,
    targets: &[Target],
    ctx: &RunContext,
    config: &Config,
    out: &mut String,
) {
    if item.path.is_empty() {
        out.push_str("  （実行するパスがありません）\r\n");
        return;
    }

    // :dir を書いていない項目には実行ファイルの場所が入る。値だけ見せられても
    // なぜそこなのか読み取れないので、既定で埋まったことを添える
    let default_dir = if item.working_dir.is_empty() {
        "  （:dir 未指定のため実行ファイルの場所）"
    } else {
        ""
    };

    let base = PathPlaceholders::from_path(&targets[0].path);

    // 入力欄は出さずに既定値で埋める。プレビューがダイアログを出したら、
    // 「起動せずに確かめる」という目的が成り立たない
    write_prompts(item, &base, ctx, out);

    // 確認は項目に対して 1 回なので、起動ごとの繰り返しの外に出す
    if let Some(message) = &item.confirm {
        let shown = if message.is_empty() {
            "あり（メッセージなし）".to_string()
        } else {
            base.replace(message, ctx)
        };
        out.push_str(&format!("  {}  {}\r\n", LABEL_CONFIRM, shown));
    }

    let invocations = resolve_invocations(item, targets, ctx);
    let total = invocations.len();

    // 間隔も項目に対する設定なので、起動ごとの繰り返しの外に出す
    write_delay(config.delay_of(item), total, out);

    for (index, invocation) in invocations.iter().enumerate() {
        // まとめて渡す項目は 1 回、そうでなければ対象の数だけ起動される
        if total > 1 {
            out.push_str(&format!("  [{}/{}]\r\n", index + 1, total));
        }

        out.push_str(&format!(
            "  {}  {}\r\n",
            LABEL_PROGRAM,
            invocation.program.display()
        ));

        if invocation.args.is_empty() {
            out.push_str(&format!("  {}  （なし）\r\n", LABEL_ARG));
        }
        for arg in &invocation.args {
            let shown = if arg.is_empty() {
                "（空文字列）"
            } else {
                arg
            };
            out.push_str(&format!("  {}  {}\r\n", LABEL_ARG, shown));
        }

        out.push_str(&format!(
            "  {}  {}{}\r\n",
            LABEL_DIR, invocation.working_dir, default_dir
        ));

        // 昇格はプロセスごとなので、何回 UAC が出るかもここから読み取れる
        if invocation.admin {
            out.push_str(&format!(
                "  {}  管理者（:admin。起動時に確認が出ます）\r\n",
                LABEL_ADMIN
            ));
        }
    }
}

/// 起動の間隔と、進行状況を出すかどうかを書き出す
///
/// 出すかどうかの判定は `menu::shows_progress` に任せる。ここで数え直すと、
/// プレビューが「表示します」と言ったのに出ない設定が作れてしまう。
fn write_delay(delay: u32, total: usize, out: &mut String) {
    if delay == 0 || total < 2 {
        return;
    }

    let waits = total as u64 - 1;
    let seconds = (u64::from(delay) * waits) as f64 / 1000.0;
    let progress = if crate::menu::shows_progress(delay, total) {
        "進行状況を表示します"
    } else {
        "進行状況は表示しません"
    };

    out.push_str(&format!(
        "  {}  {} ミリ秒（待ち {} 回で合計およそ {:.1} 秒。{}）\r\n",
        LABEL_DELAY, delay, waits, seconds, progress
    ));
}

/// `$?{...}` を一覧にし、以降の置換で使う値を決める
///
/// 実行時は入力欄が出るが、プレビューでは出せない。既定値があればそれを、
/// 無ければ `<説明>` を代わりに入れて、コマンドラインの形が見えるようにする。
///
/// 項目ごとに入れ直すので、同じ `$?{...}` を別の項目にも書いてあれば
/// どちらの行にも出る（`RunContext` の答えは項目をまたいで残るため）。
fn write_prompts(item: &MenuItem, base: &PathPlaceholders, ctx: &RunContext, out: &mut String) {
    for prompt in crate::menu::item_prompts(item) {
        let message = base.replace(prompt.message, ctx);
        let default_value = base.replace(prompt.default_value, ctx);

        let (value, note) = if default_value.is_empty() {
            (format!("<{}>", message), "（既定値なし）")
        } else {
            (default_value, "（既定値）")
        };

        out.push_str(&format!(
            "  {}  {}{}  → {}{}\r\n",
            LABEL_PROMPT,
            message,
            rule_note(prompt.rule),
            value,
            note
        ));
        ctx.set_prompt(prompt.source, value);
    }
}

/// 入力欄に課された決まりの表示（制限なしなら空）
fn rule_note(rule: crate::prompt::Rule) -> &'static str {
    match rule {
        crate::prompt::Rule::Any => "",
        crate::prompt::Rule::Int => " [整数]",
        crate::prompt::Rule::Num => " [数値]",
        crate::prompt::Rule::Name => " [ファイル名]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use std::path::PathBuf;

    fn config_of(text: &str) -> Config {
        let parsed = parse(text);
        assert!(!parsed.has_error(), "設定にエラーがある");
        parsed.config
    }

    fn target(name: &str) -> Target {
        Target {
            file_type: ".txt".to_string(),
            path: PathBuf::from(format!("C:\\dir\\{}", name)),
        }
    }

    /// 時刻を固定した実行時コンテキスト（2026-08-15 土曜 14:03:05）
    fn ctx() -> RunContext {
        RunContext::for_test()
    }

    /// プレビューは入力欄を出せないので、既定値で埋めて形を見せる
    #[test]
    fn 入力欄を既定値で埋めて出す() {
        let config =
            config_of("[.txt]\n縮小 | C:\\Windows\\notepad.exe | -w $?{長辺=1280} -o $?{出力名}");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(
            text.contains("入力　　　　  長辺  → 1280（既定値）"),
            "{}",
            text
        );
        assert!(
            text.contains("入力　　　　  出力名  → <出力名>（既定値なし）"),
            "{}",
            text
        );
        // 引数にも同じ値が入る
        assert!(text.contains("引数　　　　  1280"), "{}", text);
        assert!(text.contains("引数　　　　  <出力名>"), "{}", text);
    }

    /// 説明と既定値の中のプレースホルダーも解決される
    #[test]
    fn 入力欄の説明と既定値のプレースホルダーを解決する() {
        let config = config_of(
            "[.txt]\n改名 | C:\\Windows\\notepad.exe | $?{$n の新しい名前=$a_$t{yyyyMMdd}}",
        );
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(
            text.contains("入力　　　　  a.txt の新しい名前  → a_20260815（既定値）"),
            "{}",
            text
        );
    }

    /// 実行前に確認が入ることも「何が起きるか」の一部なので出す
    #[test]
    fn 実行前の確認を出す() {
        let config = config_of(
            "[.txt]\nメッセージあり | C:\\Windows\\notepad.exe\n :confirm $n を上書きします\nメッセージなし | C:\\Windows\\notepad.exe\n :confirm\n確認なし | C:\\Windows\\notepad.exe",
        );
        let text = report(&config, &[target("a.txt")], &ctx());

        // メッセージのプレースホルダーも解決される
        assert!(
            text.contains("実行前の確認  a.txt を上書きします"),
            "{}",
            text
        );
        assert!(
            text.contains("実行前の確認  あり（メッセージなし）"),
            "{}",
            text
        );
        // :confirm を書いていない項目には行そのものが出ない
        let 確認なし = text.split("確認なし").nth(1).expect("確認なしの節がある");
        assert!(!確認なし.contains("実行前の確認"), "{}", 確認なし);
    }

    /// 日時も解決済みで出る（書式を確かめるのはプレビューの主な用途のひとつ）
    #[test]
    fn 日時を解決して出す() {
        let config =
            config_of("[.txt]\nバックアップ | C:\\Windows\\notepad.exe | $-p_$t{yyyyMMdd}.bak");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(
            text.contains("引数　　　　  C:\\dir\\a_20260815.bak"),
            "{}",
            text
        );
    }

    #[test]
    fn 項目と解決済みのコマンドラインを出す() {
        let config = config_of("[.txt]\n開く | C:\\Windows\\notepad.exe | -x $-p.bak");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(text.contains("C:\\dir\\a.txt  (.txt)"), "{}", text);
        assert!(text.contains("開く"), "{}", text);
        assert!(
            text.contains("実行ファイル  C:\\Windows\\notepad.exe"),
            "{}",
            text
        );
        assert!(text.contains("引数　　　　  -x"), "{}", text);
        assert!(text.contains("引数　　　　  C:\\dir\\a.bak"), "{}", text);
        assert!(text.contains("1 項目"), "{}", text);
    }

    /// :dir を書いていない項目は実行ファイルの親が作業フォルダになる。
    /// 値だけでは理由が読み取れないので、既定であることを添える
    #[test]
    fn 作業フォルダが既定かどうかを区別して出す() {
        let config = config_of(
            "[.txt]\n既定 | C:\\Windows\\notepad.exe\n指定 | C:\\Windows\\notepad.exe\n :dir C:\\work",
        );
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(
            text.contains("作業フォルダ  C:\\Windows  （:dir 未指定のため実行ファイルの場所）"),
            "{}",
            text
        );
        assert!(text.contains("作業フォルダ  C:\\work\r\n"), "{}", text);
    }

    #[test]
    fn サブメニューは親の名前をつなげる() {
        let config = config_of("[.txt]\n変換\n> PNG に変換 | C:\\Windows\\notepad.exe");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(text.contains("変換 > PNG に変換"), "{}", text);
        // 親自身は実行されないので数えない
        assert!(text.contains("1 項目"), "{}", text);
    }

    /// 個別実行は対象の数だけ、まとめて渡す項目は 1 回だけ起動される
    #[test]
    fn 複数選択では起動の回数がわかる() {
        let config = config_of(
            "[.txt]\n個別 | C:\\Windows\\notepad.exe\n+ まとめて | C:\\Windows\\notepad.exe",
        );
        let text = report(&config, &[target("a.txt"), target("b.txt")], &ctx());

        assert!(text.contains("[1/2]"), "{}", text);
        assert!(text.contains("[2/2]"), "{}", text);
        // まとめて渡す方は 1 回の起動に両方のパスが並ぶ
        let batch = text.split("まとめて").nth(1).expect("まとめての節がある");
        assert!(!batch.contains("[1/2]"), "{}", batch);
        assert!(batch.contains("C:\\dir\\a.txt"), "{}", batch);
        assert!(batch.contains("C:\\dir\\b.txt"), "{}", batch);
    }

    #[test]
    fn セパレーターは出さない() {
        let config =
            config_of("[.txt]\nA | C:\\Windows\\notepad.exe\n---\nB | C:\\Windows\\notepad.exe");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(!text.contains("---"), "{}", text);
        assert!(text.contains("2 項目"), "{}", text);
    }

    #[test]
    fn 引数を空にした項目は引数なしと出る() {
        let config = config_of("[.txt]\n開く | C:\\Windows\\notepad.exe |");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(text.contains("引数　　　　  （なし）"), "{}", text);
    }

    /// 進行状況が出るかどうかは起動前に決まるので、プレビューで先に読める
    #[test]
    fn 起動の間隔と進行状況の有無を出す() {
        let config = config_of("[.txt]\n開く | C:\\Windows\\notepad.exe\n :delay 300");
        let targets = [target("a.txt"), target("b.txt"), target("c.txt")];
        let text = report(&config, &targets, &ctx());

        assert!(
            text.contains(
                "起動の間隔　  300 ミリ秒（待ち 2 回で合計およそ 0.6 秒。進行状況は表示しません）"
            ),
            "{}",
            text
        );
    }

    #[test]
    fn 待ちが長ければ進行状況を出すと書く() {
        let config = config_of("[extrun]\ndelay = 500\n[.txt]\n開く | C:\\Windows\\notepad.exe");
        let targets = [target("a.txt"), target("b.txt"), target("c.txt")];
        let text = report(&config, &targets, &ctx());

        assert!(
            text.contains("合計およそ 1.0 秒。進行状況を表示します"),
            "{}",
            text
        );
    }

    /// 待ちが起きない組み合わせで間隔の行を出すと、効くように読めてしまう
    #[test]
    fn 間隔が効かないときは行を出さない() {
        let 一つだけ = config_of("[.txt]\n開く | C:\\Windows\\notepad.exe\n :delay 300");
        let text = report(&一つだけ, &[target("a.txt")], &ctx());
        assert!(!text.contains("起動の間隔"), "{}", text);

        let 指定なし = config_of("[.txt]\n開く | C:\\Windows\\notepad.exe");
        let text = report(&指定なし, &[target("a.txt"), target("b.txt")], &ctx());
        assert!(!text.contains("起動の間隔"), "{}", text);
    }

    #[test]
    fn 適用できる項目がなければその旨を出す() {
        let config = config_of("[.png]\nA | C:\\Windows\\notepad.exe");
        let text = report(&config, &[target("a.txt")], &ctx());

        assert!(
            text.contains("適用できるメニュー項目がありません"),
            "{}",
            text
        );
    }
}
