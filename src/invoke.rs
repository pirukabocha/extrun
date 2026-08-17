/*!
起動 1 回ぶんの内容を組み立てる

**実行経路（`menu::execute_command`）と `--preview` の両方がここを通る。**
コマンドラインの組み立てを他所に書くと、プレビューが嘘をつくようになる。
*/

use crate::Target;
use crate::config::MenuItem;
use crate::placeholder::{PathPlaceholders, RunContext};
use std::path::{Path, PathBuf};
/// 起動 1 回ぶんの内容
///
/// 実行と `--preview` の両方がここを通る。表示しているものと実際に起動される
/// ものがずれてはいけないので、組み立ては `resolve_invocations` の 1 か所に集める。
pub struct Invocation {
    /// 起動する実行ファイル
    pub program: PathBuf,
    /// 置換を解決済みの引数
    pub args: Vec<String>,
    /// 作業フォルダ（解決済み。未指定だった場合は実行ファイルの親）
    pub working_dir: String,
    /// 管理者として起動するか（`:admin`）
    pub admin: bool,
}

/// 項目と対象から、起動されるプロセスを組み立てる
///
/// `+`（まとめて渡す）なら 1 つ、そうでなければ `targets` と同じ順・同じ個数を返す。
///
/// `ctx` は呼び出し側で 1 回だけ作って渡す。ここで作ると対象ごとに時刻を取り直す
/// ことになり、複数選択して個別に起動したときに `$t{ss}` がずれる。
pub fn resolve_invocations(
    item: &MenuItem,
    targets: &[Target],
    ctx: &RunContext,
) -> Vec<Invocation> {
    if targets.is_empty() {
        return Vec::new();
    }

    let exe_path = PathBuf::from(&item.path);
    let working_dir = resolve_working_dir(item, &exe_path, targets, ctx);

    if item.all_mode {
        return vec![Invocation {
            args: all_mode_args(&item.args, targets, ctx),
            program: exe_path,
            working_dir,
            admin: item.admin,
        }];
    }

    targets
        .iter()
        .map(|target| Invocation {
            program: exe_path.clone(),
            args: PathPlaceholders::from_path(&target.path).replace_args(&item.args, ctx),
            working_dir: working_dir.clone(),
            admin: item.admin,
        })
        .collect()
}

/// 作業フォルダを解決する
///
/// プレースホルダーは最初の対象を基準にする。未指定なら実行ファイルの親ディレクトリ。
///
/// **`notepad.exe` のように区切りを含まない名前では、親が「空文字列」として
/// 取れる**（`None` ではない）。そのまま作業フォルダに渡すと `CreateProcess` が
/// エラー 123（構文が違う）で失敗するので、空なら現在のフォルダに倒す。
fn resolve_working_dir(
    item: &MenuItem,
    exe_path: &Path,
    targets: &[Target],
    ctx: &RunContext,
) -> String {
    if item.working_dir.is_empty() {
        return exe_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|dir| !dir.is_empty())
            .unwrap_or_else(|| ".".to_string());
    }

    PathPlaceholders::from_path(&targets[0].path).replace(&item.working_dir, ctx)
}

/// `+`（まとめて渡す）の引数を組み立てる
///
/// 引数がちょうど `$p` のところに全パスを展開する。`$p` がどこにも無ければ末尾に足す。
///
/// **引数欄を空にしてあれば引数なしで起動する。** 欄を空にする（行末を `|` で
/// 終える）のと、欄ごと省略する（`$p` が渡る）のは仕様書でも区別している。
/// かつては `+` のときだけ空欄でも全パスが末尾に付き、同じ書き方が場所によって
/// 逆の意味になっていた。
fn all_mode_args(base_args: &[String], targets: &[Target], ctx: &RunContext) -> Vec<String> {
    if base_args.is_empty() {
        return Vec::new();
    }

    let placeholder_count = base_args.iter().filter(|arg| arg.as_str() == "$p").count();
    // エスケープを解する判定を使う。素の `contains("$p")` だと `^$path` のような
    // 書き方を「`$p` がある」と誤解し、末尾へのパス追加を止めてしまう
    let has_path_placeholder = base_args
        .iter()
        .any(|arg| crate::text::has_path_placeholder(arg));
    let extra_path_args = if has_path_placeholder {
        placeholder_count.saturating_mul(targets.len().saturating_sub(1))
    } else {
        targets.len()
    };
    let mut final_args = Vec::with_capacity(base_args.len() + extra_path_args);

    let placeholders = PathPlaceholders::from_path(&targets[0].path);
    for arg in base_args {
        if arg == "$p" {
            for target in targets {
                final_args.push(target.path.to_string_lossy().to_string());
            }
        } else {
            final_args.push(placeholders.replace(arg, ctx));
        }
    }

    if !has_path_placeholder {
        for target in targets {
            final_args.push(target.path.to_string_lossy().to_string());
        }
    }

    final_args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    /// `notepad.exe` のように区切りを含まない名前では、親が `None` ではなく
    /// **空文字列**として取れる。そのまま作業フォルダに渡すと `CreateProcess` が
    /// エラー 123（構文が違う）で失敗するので、現在のフォルダに倒す
    #[test]
    fn 区切りのない実行ファイルでも作業フォルダが空にならない() {
        let config = parse("[.txt]\nA | notepad.exe").config;
        let targets = vec![Target::from_path(PathBuf::from("C:\\x\\y.txt"))];
        let invocations = resolve_invocations(&config.apps[0], &targets, &RunContext::for_test());
        assert_eq!(invocations[0].working_dir, ".");
    }

    /// 引数欄を空にする（行末を `|` で終える）と引数なし。欄ごと省略したとき
    /// （`$p` が渡る）とは区別する。**`+` でも意味を揃える** — かつては `+` の
    /// ときだけ空欄でも全パスが末尾に付き、同じ書き方が逆の意味になっていた
    #[test]
    fn まとめて渡す項目でも空の引数欄は引数なし() {
        let config = parse("[.txt]\n+ A | C:\\a.exe |").config;
        let targets = vec![
            Target::from_path(PathBuf::from("C:\\x\\1.txt")),
            Target::from_path(PathBuf::from("C:\\x\\2.txt")),
        ];
        let invocations = resolve_invocations(&config.apps[0], &targets, &RunContext::for_test());

        assert_eq!(invocations.len(), 1, "+ は 1 プロセス");
        assert!(invocations[0].args.is_empty(), "{:?}", invocations[0].args);
    }

    /// `^$path` を「`$p` がある」と誤解すると、末尾へのパス追加が止まり、
    /// 対象がどこにも渡らないままコマンドが起動する（`--check` も黙っていた）
    #[test]
    fn エスケープした_p_があっても全パスは末尾に付く() {
        let config = parse("[.txt]\n+ A | C:\\a.exe | -c ^$path").config;
        let targets = vec![
            Target::from_path(PathBuf::from("C:\\x\\1.txt")),
            Target::from_path(PathBuf::from("C:\\x\\2.txt")),
        ];
        let invocations = resolve_invocations(&config.apps[0], &targets, &RunContext::for_test());

        assert_eq!(invocations.len(), 1, "+ は 1 プロセス");
        assert_eq!(
            invocations[0].args,
            vec!["-c", "$path", "C:\\x\\1.txt", "C:\\x\\2.txt"]
        );
    }

    #[test]
    fn 管理者指定は起動の組み立てに伝わる() {
        let config = parse("[.txt]\nA | C:\\Windows\\notepad.exe\n :admin").config;
        let targets = vec![Target::from_path(PathBuf::from("C:\\x\\y.txt"))];
        let invocations = resolve_invocations(&config.apps[0], &targets, &RunContext::for_test());
        assert!(invocations[0].admin);
    }
}
