/*!
起動 1 回ぶんの内容を組み立てる

**実行経路（`menu::execute_command`）と `--preview` の両方がここを通る。**
コマンドラインの組み立てを他所に書くと、プレビューが嘘をつくようになる。
*/

use crate::config::MenuItem;
use crate::placeholder::{PathPlaceholders, RunContext};
use crate::Target;
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
            .unwrap_or_else(|| ".".to_string());
    }

    PathPlaceholders::from_path(&targets[0].path).replace(&item.working_dir, ctx)
}

/// `+`（まとめて渡す）の引数を組み立てる
///
/// 引数がちょうど `$p` のところに全パスを展開する。`$p` がどこにも無ければ末尾に足す。
fn all_mode_args(base_args: &[String], targets: &[Target], ctx: &RunContext) -> Vec<String> {
    let placeholder_count = base_args.iter().filter(|arg| arg.as_str() == "$p").count();
    let has_path_placeholder = base_args.iter().any(|arg| arg.contains("$p"));
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
    #[test]
    fn 管理者指定は起動の組み立てに伝わる() {
        let config = parse("[.txt]\nA | C:\\Windows\\notepad.exe\n :admin").config;
        let targets = vec![Target::from_path(PathBuf::from("C:\\x\\y.txt"))];
        let invocations = resolve_invocations(&config.apps[0], &targets, &RunContext::for_test());
        assert!(invocations[0].admin);
    }
}
