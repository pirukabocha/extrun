/*!
`extrun.exe --check` の実装

設定ファイルをパースし、書式のエラーと実行ファイルの存在確認の結果を
行番号付きでコンソールに出力する。メニューの構築とは独立している。

「そのパスに対して実際に何が起動されるか」を見るのは `preview.rs` の担当。
*/

use crate::config::{Config, Diag, MenuItem, Severity};
use crate::console;
use std::path::Path;

/// 設定ファイルを検証して結果を出力し、終了コードを返す
///
/// エラーがあれば 1、警告だけ、または問題なしなら 0。バッチや CI から
/// 設定ファイルの検証に使えるようにするための区別。
pub fn run(config_path: &Path) -> i32 {
    let report = report(config_path);
    console::print(&report.text);
    if report.has_error { 1 } else { 0 }
}

/// 検証結果
struct Report {
    text: String,
    has_error: bool,
}

/// 検証結果を整形する
fn report(config_path: &Path) -> Report {
    let file_name = config_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| config_path.display().to_string());

    let parsed = match Config::load(config_path) {
        Ok(parsed) => parsed,
        Err(message) => {
            return Report {
                text: format!("{}\r\n", message.replace('\n', "\r\n")),
                has_error: true,
            };
        }
    };

    let mut diags = parsed.diags;
    collect_item_diags(&parsed.config.apps, &mut diags);
    diags.sort_by_key(|diag| diag.line);

    if diags.is_empty() {
        return Report {
            text: format!("{}: 問題は見つかりませんでした\r\n", file_name),
            has_error: false,
        };
    }

    let has_error = diags.iter().any(|diag| diag.severity == Severity::Error);
    let mut out = format!("{}: {} 件の問題\r\n\r\n", file_name, diags.len());
    for diag in &diags {
        let mark = match diag.severity {
            Severity::Error => "エラー",
            Severity::Warning => "警告　",
        };
        out.push_str(&format!(
            "{:>4}行目  {}  {}\r\n",
            diag.line, mark, diag.message
        ));
    }

    Report {
        text: out,
        has_error,
    }
}

/// 項目ごとの問題を集める（セパレーターとサブメニューの親は除く）
fn collect_item_diags(items: &[MenuItem], diags: &mut Vec<Diag>) {
    // 重複だけは兄弟をまとめて見る必要があるので、潜る前にこの階層を調べる
    warn_duplicate_accesskeys(items, diags);

    for item in items {
        // アイコンはサブメニューの親にも付くので、実行の可否より先に見る
        warn_missing_icon(item, diags);

        if item.has_submenu() {
            warn_unreachable_fields(item, "サブメニューの親", diags);
            collect_item_diags(&item.submenu, diags);
            continue;
        }

        if item.is_separator() {
            warn_unreachable_fields(item, "セパレーター", diags);
            continue;
        }

        if item.path.is_empty() {
            diags.push(Diag::warning(
                item.line,
                format!("実行するパスがありません: {}", item.name),
            ));
            continue;
        }

        // 相対パスは実行時のカレント次第なので確認しない。ただし展開されなかった
        // `%NAME%` が残っているパスは、絶対と判定されないだけで解決もしないので見る
        let path = Path::new(&item.path);
        let checkable = path.is_absolute() || crate::text::has_unexpanded_env(&item.path);
        if checkable && !path.exists() {
            diags.push(Diag::warning(
                item.line,
                format!("実行ファイルが見つかりません: {}", item.path),
            ));
        }

        // 起動を試みた時点でも同じ案内を出すが、書いた時点で気づけるほうが早い
        if crate::launch::needs_interpreter(path) {
            diags.push(Diag::warning(
                item.line,
                format!("{}: {}", crate::launch::INTERPRETER_HINT, item.path),
            ));
        }

        warn_embedded_path_placeholder(item, diags);
        warn_delay_on_all_mode(item, diags);
    }
}

/// `:icon` から実際にアイコンを取り出せるか調べる
///
/// ファイルの有無だけでなく**取り出しまで試す**。番号が範囲の外だと、パスは
/// 正しいのにアイコンだけ出ないという分かりにくい結果になるため。
///
/// 出なくてもメニューそのものは表示されるので、警告にとどめる。相対パスは
/// 実行時のカレントに依存するので、実行ファイルと同じく確認しない。
fn warn_missing_icon(item: &MenuItem, diags: &mut Vec<Diag>) {
    let Some(spec) = &item.icon else {
        return;
    };

    let path = Path::new(&spec.path);
    if !path.is_absolute() && !crate::text::has_unexpanded_env(&spec.path) {
        return;
    }

    if !path.exists() {
        diags.push(Diag::warning(
            item.line,
            format!("アイコンのファイルが見つかりません: {}", spec.path),
        ));
        return;
    }

    // 大きさは何でもよい（取り出せるかどうかだけを見る）
    match crate::icon::load(path, spec.index, 16) {
        Some(bitmap) => crate::icon::dispose(bitmap),
        None => diags.push(Diag::warning(
            item.line,
            format!(
                "アイコンを取り出せません（番号を確認してください）: {},{}",
                spec.path, spec.index
            ),
        )),
    }
}

/// 実行されない項目に、実行のときだけ効くフィールドが付いていないか調べる
///
/// サブメニューの親とセパレーターは選んでもコマンドが走らないので、確認も昇格も
/// 起きない。書いた側は「付けた」と思っているのに何も起きないので、黙って捨てない。
fn warn_unreachable_fields(item: &MenuItem, kind: &str, diags: &mut Vec<Diag>) {
    if item.confirm.is_some() {
        diags.push(Diag::warning(
            item.line,
            format!(
                "{}に :confirm があります（実行されないので確認も出ません）: {}",
                kind, item.name
            ),
        ));
    }

    if item.admin {
        diags.push(Diag::warning(
            item.line,
            format!(
                "{}に :admin があります（実行されないので効きません）: {}",
                kind, item.name
            ),
        ));
    }

    if item.delay.is_some() {
        diags.push(Diag::warning(
            item.line,
            format!(
                "{}に :delay があります（実行されないので効きません）: {}",
                kind, item.name
            ),
        ));
    }

    if item.wait {
        diags.push(Diag::warning(
            item.line,
            format!(
                "{}に :wait があります（実行されないので効きません）: {}",
                kind, item.name
            ),
        ));
    }
}

/// `+` の項目に `:delay` や `:wait` が付いていないか調べる
///
/// まとめて渡す項目は何個選んでも 1 プロセスしか起動しないので、起動と起動の
/// あいだが存在しない。間隔を空けるつもりでも、終了を待つつもりでも、待つ相手が
/// いない。書いた側はそのつもりでいるので、黙って捨てない。
fn warn_delay_on_all_mode(item: &MenuItem, diags: &mut Vec<Diag>) {
    if !item.all_mode {
        return;
    }

    for keyword in [
        item.delay.is_some().then_some(":delay"),
        item.wait.then_some(":wait"),
    ]
    .into_iter()
    .flatten()
    {
        diags.push(Diag::warning(
            item.line,
            format!(
                "+ の項目に {} があります（1 プロセスしか起動しないので効きません）: {}",
                keyword, item.name
            ),
        ));
    }
}

/// 同じ階層でアクセスキーが重複していないか調べる
///
/// Win32 のニーモニックはポップアップごとにスコープされるので、比べるのは兄弟だけ。
/// 親と子で同じ文字を使うのは問題ない。重複していると、キーを押しても実行されず
/// 候補が順に選択されるだけになるので、押しても動かないように見える。
///
/// 拡張子が違う項目どうしでも警告する。セクションが違ってもルートの項目は同じ階層に
/// 並ぶし、複数選択したときのメニューはそれぞれの和集合になるので、実際に同居しうる。
fn warn_duplicate_accesskeys(items: &[MenuItem], diags: &mut Vec<Diag>) {
    // 項目数はたかが知れているので線形探索で足りる
    let mut seen: Vec<(char, u32, &str)> = Vec::new();

    for item in items {
        let Some(key) = item.accesskey_char() else {
            continue;
        };

        match seen.iter().find(|(c, _, _)| *c == key) {
            Some((_, first_line, first_name)) => diags.push(Diag::warning(
                item.line,
                format!(
                    "アクセスキー {} が {}行目の「{}」と重複しています: {}",
                    key, first_line, first_name, item.name
                ),
            )),
            None => seen.push((key, item.line, &item.name)),
        }
    }
}

/// `+` の項目で `$p` が引数の一部になっていないか調べる
///
/// まとめて渡すときに全パスへ展開されるのは、引数がちょうど `$p` のときだけ。
/// `-i$p` のように埋め込むと最初の 1 つしか渡らず、残りを黙って取りこぼす。
fn warn_embedded_path_placeholder(item: &MenuItem, diags: &mut Vec<Diag>) {
    if !item.all_mode || item.args.iter().any(|arg| arg == "$p") {
        return;
    }

    if item.args.iter().any(|arg| has_path_placeholder(arg)) {
        diags.push(Diag::warning(
            item.line,
            format!(
                "+ の項目で $p が引数の一部になっています（全パスに展開されるのは引数が $p だけのとき）: {}",
                item.name
            ),
        ));
    }
}

/// エスケープされていない `$p` を含むか（`$-p` は別のプレースホルダーなので除く）
fn has_path_placeholder(arg: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    fn diags_of(text: &str) -> Vec<Diag> {
        let parsed = parse(text);
        let mut diags = parsed.diags;
        collect_item_diags(&parsed.config.apps, &mut diags);
        diags
    }

    #[test]
    fn パスのない項目を警告する() {
        let diags = diags_of("[.txt]\nパスなし");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("実行するパスがありません"))
        );
    }

    /// パスの先頭に書いた変数の綴りを間違えると、展開されないまま残る。
    /// このパスは絶対パスと判定されないので、存在確認から漏れないようにする
    #[test]
    fn 展開されなかった環境変数を含むパスを警告する() {
        let diags = diags_of("[.txt]\nX | %EXTRUN_NO_SUCH_VAR%\\Programs\\app.exe");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("実行ファイルが見つかりません")),
            "{:?}",
            diags
        );
    }

    #[test]
    fn 展開された環境変数のパスは警告しない() {
        let diags = diags_of("[.txt]\nX | %SystemRoot%\\notepad.exe");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    /// CreateProcess はスクリプトを起動できないので、書いた時点で知らせる
    #[test]
    fn スクリプトを直接指定した項目を警告する() {
        let diags = diags_of("[.txt]\nX | C:\\tools\\run.ps1");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("スクリプトは直接起動できません")),
            "{:?}",
            diags
        );
    }

    #[test]
    fn スクリプトの判定は大文字小文字を区別しない() {
        let diags = diags_of("[.txt]\nX | C:\\tools\\RUN.PS1");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("スクリプトは直接起動できません")),
            "{:?}",
            diags
        );
    }

    /// 標準ライブラリが `.bat` / `.cmd` だけを `cmd.exe /c` に差し替えるので、
    /// この 2 つは直接書いても動く。警告してはいけない
    #[test]
    fn バッチファイルは警告しない() {
        for path in [
            "C:\\tools\\run.bat",
            "C:\\tools\\run.cmd",
            "C:\\tools\\RUN.BAT",
        ] {
            let diags = diags_of(&format!("[.txt]\nX | {}", path));
            assert!(
                !diags
                    .iter()
                    .any(|d| d.message.contains("スクリプトは直接起動できません")),
                "{}: {:?}",
                path,
                diags
            );
        }
    }

    #[test]
    fn インタプリタ経由なら警告しない() {
        let diags = diags_of(
            "[.txt]\nX | C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe | -File C:\\tools\\run.ps1 $p",
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("スクリプトは直接起動できません")),
            "{:?}",
            diags
        );
    }

    /// 実行されない項目に付けた :confirm は黙って捨てられるので警告する
    #[test]
    fn 実行されない項目の確認を警告する() {
        let 親 = diags_of("[.txt]\n親\n :confirm よろしいですか\n> 子 | C:\\Windows\\notepad.exe");
        assert!(
            親.iter()
                .any(|d| d.message.contains("サブメニューの親に :confirm")),
            "{:?}",
            親
        );

        let 区切り = diags_of(
            "[.txt]\nA | C:\\Windows\\notepad.exe\n---\n :confirm ためし\nB | C:\\Windows\\notepad.exe",
        );
        assert!(
            区切り
                .iter()
                .any(|d| d.message.contains("セパレーターに :confirm")),
            "{:?}",
            区切り
        );
    }

    /// 実行される項目に付いていれば当然ながら警告しない
    #[test]
    fn 実行される項目の確認は警告しない() {
        let diags = diags_of("[.txt]\nA | C:\\Windows\\notepad.exe\n :confirm よろしいですか");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn サブメニューの親とセパレーターは警告しない() {
        let diags = diags_of("[.txt]\n親\n> ---\n> 子 | C:\\Windows\\notepad.exe");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn 存在しない絶対パスを警告する() {
        let diags = diags_of("[.txt]\nA | C:\\存在しないフォルダ\\存在しない.exe");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("実行ファイルが見つかりません"))
        );
    }

    #[test]
    fn 相対パスは存在確認しない() {
        let diags = diags_of("[.txt]\nA | notepad.exe");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    /// `+` の項目に埋め込まれた `$p` は最初の 1 つしか渡らない
    #[test]
    fn まとめて実行で_p_が引数の一部なら警告する() {
        let diags = diags_of("[.txt]\n+ A | notepad.exe | -i$p");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("$p が引数の一部になっています")),
            "{:?}",
            diags
        );
    }

    #[test]
    fn まとめて実行で_p_が独立していれば警告しない() {
        // 引数がちょうど $p のとき、$p を含まないとき、エスケープしたときは警告しない
        for args in ["-t7z $d\\a.7z $p", "-o $d", "^$p"] {
            let diags = diags_of(&format!("[.txt]\n+ A | notepad.exe | {}", args));
            assert!(diags.is_empty(), "{}: {:?}", args, diags);
        }
    }

    #[test]
    fn 個別実行なら_p_が引数の一部でも警告しない() {
        let diags = diags_of("[.txt]\nA | notepad.exe | -i$p");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    // -----------------------------------------------------------------
    // アクセスキー
    // -----------------------------------------------------------------

    fn 重複の警告(text: &str) -> Vec<String> {
        diags_of(text)
            .iter()
            .filter(|d| d.message.contains("重複しています"))
            .map(|d| d.message.clone())
            .collect()
    }

    #[test]
    fn 同じ階層のアクセスキーの重複を警告する() {
        let warnings = 重複の警告("[.txt]\n&Alpha | notepad.exe\n&Apple | notepad.exe");
        assert_eq!(warnings.len(), 1, "{:?}", warnings);
        assert!(warnings[0].contains("アクセスキー A"), "{:?}", warnings);
    }

    #[test]
    fn アクセスキーの重複は大文字小文字を区別しない() {
        // Win32 のニーモニックは大小を区別しない
        let warnings = 重複の警告("[.txt]\n&Alpha | notepad.exe\n&apple | notepad.exe");
        assert_eq!(warnings.len(), 1, "{:?}", warnings);
    }

    /// キーはメニューごとにスコープされるので、親と子で同じ文字を使える
    #[test]
    fn 階層が違えばアクセスキーは重複しない() {
        let warnings = 重複の警告("[.txt]\n&Zip\n> &Zip 個別 | notepad.exe");
        assert!(warnings.is_empty(), "{:?}", warnings);
    }

    /// セクションが違ってもルートの項目は同じメニューに並ぶ
    /// （複数選択したときのメニューはそれぞれの和集合になる）
    #[test]
    fn セクションが違ってもルートなら重複を警告する() {
        let warnings =
            重複の警告("[.txt]\n&Alpha | notepad.exe\n[folder]\n&Apple | notepad.exe");
        assert_eq!(warnings.len(), 1, "{:?}", warnings);
    }

    /// サブメニューの親は実行されないが、キーは押されるので判定に入る
    #[test]
    fn サブメニューの親も重複判定に入る() {
        let warnings = 重複の警告("[.txt]\n&Zip\n> 子 | notepad.exe\n&Zoom | notepad.exe");
        assert_eq!(warnings.len(), 1, "{:?}", warnings);
    }

    #[test]
    fn アクセスキーのない項目は重複しない() {
        let warnings = 重複の警告("[.txt]\nA | notepad.exe\nB | notepad.exe");
        assert!(warnings.is_empty(), "{:?}", warnings);
    }

    /// `+` は何個選んでも 1 プロセスなので、起動と起動のあいだが存在しない
    #[test]
    fn まとめて渡す項目の間隔を警告する() {
        let diags = diags_of("[.txt]\n+ A | %SystemRoot%\\notepad.exe\n :delay 300");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("+ の項目に :delay があります")),
            "{:?}",
            diags
        );
    }

    #[test]
    fn まとめて渡さない項目の間隔は警告しない() {
        let diags = diags_of("[.txt]\nA | %SystemRoot%\\notepad.exe\n :delay 300");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    #[test]
    fn 実行されない項目の間隔を警告する() {
        for text in [
            "[.txt]\n親\n :delay 300\n> 子 | %SystemRoot%\\notepad.exe",
            "[.txt]\n---\n :delay 300",
        ] {
            let diags = diags_of(text);
            assert!(
                diags
                    .iter()
                    .any(|d| d.message.contains(":delay があります（実行されないので")),
                "{}: {:?}",
                text,
                diags
            );
        }
    }

    /// `+` は 1 プロセスしか起動しないので、待つ相手がいない
    #[test]
    fn まとめて渡す項目の終了待ちを警告する() {
        let diags = diags_of("[.txt]\n+ A | %SystemRoot%\\notepad.exe\n :wait");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("+ の項目に :wait があります")),
            "{:?}",
            diags
        );
    }

    #[test]
    fn まとめて渡さない項目の終了待ちは警告しない() {
        let diags = diags_of("[.txt]\nA | %SystemRoot%\\notepad.exe\n :wait");
        assert!(diags.is_empty(), "{:?}", diags);
    }

    /// `:delay` と `:wait` を両方書いた `+` の項目では、どちらも効かないので
    /// 2 件とも知らせる（片方だけ消しても状況が変わらない）
    #[test]
    fn まとめて渡す項目では間隔と終了待ちの両方を警告する() {
        let diags = diags_of("[.txt]\n+ A | %SystemRoot%\\notepad.exe\n :delay 300\n :wait");
        let warnings: Vec<&str> = diags
            .iter()
            .filter(|d| d.message.contains("+ の項目に"))
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(warnings.len(), 2, "{:?}", warnings);
    }

    #[test]
    fn 実行されない項目の終了待ちを警告する() {
        for text in [
            "[.txt]\n親\n :wait\n> 子 | %SystemRoot%\\notepad.exe",
            "[.txt]\n---\n :wait",
        ] {
            let diags = diags_of(text);
            assert!(
                diags
                    .iter()
                    .any(|d| d.message.contains(":wait があります（実行されないので")),
                "{}: {:?}",
                text,
                diags
            );
        }
    }
}
