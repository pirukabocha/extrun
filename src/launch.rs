/*!
組み立てたプロセスを起動する

`:delay`（起動の間隔を空ける）と `:wait`（前の 1 つが終わるまで次を起動しない）
の実体。待ちそのものは `progress.rs` のダイアログのモーダルループが担い、
こちらは起動と終了の判定だけを持つ（組み立ては `invoke.rs` の 1 か所に集める）。
*/

use crate::Target;
use crate::invoke::Invocation;
use crate::menu::{show_error_dialog, to_wide_string};
use crate::progress;
use std::path::Path;
use std::process::{Child, Command};
use std::ptr::null_mut;
use std::sync::Once;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
/// 進行状況ダイアログを出す待ち時間の合計（ミリ秒）
///
/// これより短い待ちのためにダイアログが一瞬出て消えるほうが煩わしいので、
/// 下回るときは黙って待つ。**判定は起動より前に確定する**ので、`--preview` に
/// 「進行状況を表示します」と書ける。
const PROGRESS_THRESHOLD_MS: u64 = 1_000;

/// 起動の進み具合
#[derive(Default)]
pub(crate) struct Run {
    /// 起動を試みた数（残りを数えるのに使う）
    pub(crate) attempted: usize,
    /// 実際に起動できた数
    pub(crate) started: usize,
    /// 起動できなかった理由
    pub(crate) reasons: Vec<String>,
    /// 進行状況を出していて、途中で止まったときの理由
    pub(crate) interrupted: Option<progress::Outcome>,
}

/// 進行状況ダイアログを出すかどうかを決める
///
/// `:wait` は**2 件以上なら必ず出す**。待ち時間が起動したプロセス側に委ねられ、
/// 事前に合計を計算できないため。ここでしきい値を持ち出すと、中止も一時停止も
/// できないまま何分も待たされる設定が作れてしまう。上限を置かないことと
/// 引き換えの約束なので、条件を緩めるときはこの理由ごと見直すこと。
pub fn shows_progress(wait: bool, delay: u32, total: usize) -> bool {
    if total < 2 {
        return false;
    }
    if wait {
        return true;
    }

    delay > 0 && u64::from(delay) * (total as u64 - 1) >= PROGRESS_THRESHOLD_MS
}

/// 組み立てたプロセスを順に起動する
///
/// `delay` が 0 なら間を空けずに起動する（`:delay` を書くまでの従来どおり）。
/// 値があるときは起動と起動のあいだを空け、合計が長くなるときだけ進行状況を
/// 見せる。`wait` が立っていれば、直前のプロセスが終わるまで次を起動しない。
/// **待ち時間の実体は進行状況ダイアログのモーダルループ**で、そちらを使うときは
/// `thread::sleep` を通らない（眠るとメッセージを汲めなくなる）。
pub(crate) fn launch_all(name: &str, invocations: &[Invocation], delay: u32, wait: bool) -> Run {
    let total = invocations.len();
    let mut run = Run::default();
    // `:wait` のときだけ、直前に起動したプロセスを終了の判定のために持つ。
    // 持つのは 1 つだけでよい（待つ相手は常に直前の 1 つ）
    let mut running: Option<Running> = None;

    // 1 歩ぶんの起動。`:wait` では「起動してよいか」の問い合わせを兼ねる
    let mut launch = |index: usize| -> progress::Step {
        if running.as_mut().is_some_and(Running::alive) {
            return progress::Step::Busy;
        }
        // ハンドルはここで閉じる（次を起動する前に手放す）
        running = None;

        let invocation = &invocations[index];

        // `attempted` を進めるのは**起動を試み終えてから**。先に進めると、UAC を
        // 断られた 1 つが「起動済み」にも「残り」にも数えられず、要約の件数が
        // 合わなくなる（10 件中 2 件起動・残り 7 件、のように 1 件消える）。
        // クリップボードの一覧からも漏れるので、やり直したい当人が拾えない。
        match spawn_command(
            &invocation.program,
            &invocation.args,
            &invocation.working_dir,
            invocation.admin,
            wait,
        ) {
            Ok(Launch::Started(handle)) => {
                running = handle;
                run.attempted = index + 1;
                run.started += 1;
                progress::Step::Started
            }
            // 昇格を断られたら、残りの対象も起動しない。**この 1 つも「残り」に
            // 含める** — 起動していないのだから、やり直す対象はここから後ろになる
            Ok(Launch::Cancelled) => progress::Step::Stop,
            // 起動できなかった理由は集めておいて、あとで 1 枚にまとめる。
            // 起動していないので待つ相手もいない（次へ進む）
            Err(reason) => {
                run.attempted = index + 1;
                run.reasons.push(reason);
                progress::Step::Started
            }
        }
    };

    let mut interrupted = None;

    if shows_progress(wait, delay, total) {
        match progress::run(name, total, delay, wait, &mut launch) {
            progress::Outcome::Finished => {}
            // ダイアログを出せなかったとき。`:delay` だけなら、せめて順番を守って
            // 起動する（待ち時間には上限があるので、無表示でも必ず終わる）。
            //
            // **`:wait` では 1 つも起動しない。** 待ちの長さは起動したプロセス側に
            // 委ねられていて上限が無く、その引き換えに「中止と一時停止の手立てを
            // 常に持たせる」と約束している。ダイアログが無ければその約束を果たせず、
            // 無表示・中止不可のまま固まる。約束を守れないなら実行しない方に倒す。
            progress::Outcome::Failed if wait => show_error_dialog(
                "エラー",
                "進行状況ダイアログを表示できないため、実行を取りやめました。\n\n\
                 :wait は前のプロセスが終わるまで次を起動しないため、\
                 途中で中止する手立てが無いまま待ち続けることになります。",
            ),
            progress::Outcome::Failed => launch_in_order(total, delay, &mut launch),
            stopped => interrupted = Some(stopped),
        }
    } else {
        launch_in_order(total, delay, &mut launch);
    }

    // ここから先で `launch` を使わないので、`run` の借用が外れる
    run.interrupted = interrupted;
    run
}

/// 順に起動する（進行状況は出さない）
///
/// 進行状況ダイアログを出さない経路。`:wait` でここに来るのは対象が 1 つの
/// ときと、ダイアログを組み立てられなかったときだけなので、`Busy` は眠って
/// 待つ（汲むべきメッセージが無いので `thread::sleep` でよい）。
fn launch_in_order(total: usize, delay: u32, launch: &mut dyn FnMut(usize) -> progress::Step) {
    let mut index = 0;

    while index < total {
        match launch(index) {
            progress::Step::Started => {
                index += 1;
                // 待つのは起動と起動の「あいだ」。最後の 1 つのあとは待たない
                if index < total && delay > 0 {
                    std::thread::sleep(Duration::from_millis(u64::from(delay)));
                }
            }
            progress::Step::Busy => {
                std::thread::sleep(Duration::from_millis(u64::from(progress::POLL_INTERVAL_MS)))
            }
            progress::Step::Stop => break,
        }
    }
}

/// まだ起動していない対象のパス
///
/// `resolve_invocations` は個別実行のとき対象と同じ順・同じ個数を返すので、
/// 起動を試みた数がそのまま境目になる。`+`（まとめて渡す）は 1 プロセスなので
/// 間隔そのものが効かず、ここには来ない。
pub(crate) fn remaining_paths(
    targets: &[Target],
    invocations: &[Invocation],
    attempted: usize,
) -> Vec<String> {
    if invocations.len() != targets.len() {
        return Vec::new();
    }

    targets[attempted.min(targets.len())..]
        .iter()
        .map(|target| target.path.to_string_lossy().to_string())
        .collect()
}

/// 起動の結果（`Cancelled` は UAC を断られた場合）
enum Launch {
    /// 起動した。`:wait` のときだけ、終了を見るための持ち手が付く
    Started(Option<Running>),
    Cancelled,
}

/// 終了を待つために持っておく、起動したプロセス（`:wait`）
///
/// **`:wait` を書いた項目でしか作らない。** 常に持つと、`:admin` の経路で
/// ハンドルを閉じる責任が全経路に増えるうえ、ExtRun が「起動したら手を離す」
/// 道具であることが実装から読み取れなくなる。
enum Running {
    /// `CreateProcess` 経由（`std` が閉じてくれる）
    Child(Child),
    /// `ShellExecuteExW` 経由（`:admin`。こちらは自分で閉じる）
    Elevated(HANDLE),
}

impl Running {
    /// まだ動いているか
    ///
    /// **分からないときは「動いていない」に倒す。** 待ち続けて全体が止まる方が、
    /// 少し早く次を起動してしまうより害が大きい（`:delay` と違って上限が無い）。
    fn alive(&mut self) -> bool {
        match self {
            Running::Child(child) => matches!(child.try_wait(), Ok(None)),
            Running::Elevated(handle) => unsafe { WaitForSingleObject(*handle, 0) == WAIT_TIMEOUT },
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // `Child` は `std` が閉じる。`ShellExecuteExW` から受け取ったものだけ
        // こちらの責任になる
        if let Running::Elevated(handle) = self {
            unsafe { CloseHandle(*handle) };
        }
    }
}

/// プロセスを起動する（失敗したら理由を返す）
///
/// `keep` が立っているときだけ、終了を見るための持ち手を返す（`:wait`）。
fn spawn_command(
    exe_path: &Path,
    args: &[String],
    working_dir: &str,
    admin: bool,
    keep: bool,
) -> Result<Launch, String> {
    if admin {
        return spawn_elevated(exe_path, args, working_dir, keep);
    }

    Command::new(exe_path)
        .args(args)
        .current_dir(working_dir)
        .spawn()
        .map(|child| Launch::Started(keep.then_some(Running::Child(child))))
        .map_err(|error| error.to_string())
}

/// 管理者として起動する（`:admin`）
///
/// `CreateProcess` には昇格の仕組みが無いので、`runas` 動詞で
/// `ShellExecuteExW` を呼ぶ。`CreateProcess` 経路と違って引数を 1 本の文字列で
/// 渡すため、`join_args` が引用符を付け直す。
fn spawn_elevated(
    exe_path: &Path,
    args: &[String],
    working_dir: &str,
    keep: bool,
) -> Result<Launch, String> {
    // ShellExecuteEx はシェル拡張に処理を委譲することがあり、STA での COM の
    // 初期化が推奨されている。`:admin` を使う設定でだけ払うコストにするため、
    // 起動時ではなくここで 1 回だけ初期化する
    static COM_INIT: Once = Once::new();
    COM_INIT.call_once(|| unsafe {
        CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32);
    });

    let verb = to_wide_string("runas");
    let file = to_wide_string(&exe_path.to_string_lossy());
    let parameters = to_wide_string(&join_args(args));
    let directory = to_wide_string(working_dir);

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    // ExtRun は起動したらすぐ終了するので、これが無いと起動処理ごと打ち切られうる
    //
    // `SEE_MASK_NOCLOSEPROCESS` は `:wait` のときだけ足す。付けると hProcess が
    // 返る代わりに閉じる責任もこちらに移るので、要らない経路では持たない
    info.fMask = if keep {
        SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS
    } else {
        SEE_MASK_NOASYNC
    };
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = if args.is_empty() {
        null_mut()
    } else {
        parameters.as_ptr()
    };
    info.lpDirectory = directory.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    if unsafe { ShellExecuteExW(&mut info) } != 0 {
        // ハンドルを求めなかったとき、また求めても得られなかったときは
        // 持ち手なし（`:wait` では待たずに次へ進む）
        let handle = (keep && !info.hProcess.is_null()).then_some(Running::Elevated(info.hProcess));
        return Ok(Launch::Started(handle));
    }

    // 昇格を断るのは「実行しない」という意思表示であって失敗ではない
    match unsafe { GetLastError() } {
        ERROR_CANCELLED => Ok(Launch::Cancelled),
        code => Err(format!("管理者として起動できません (エラー {})", code)),
    }
}

/// 引数の並びを 1 本のコマンドラインにする
///
/// `ShellExecuteExW` は引数を単一の文字列で受け取るので、`CreateProcess` 経路で
/// `Command` がやっていた引用符付けを自前で行う。規則は受け取り側の
/// `CommandLineToArgvW` に合わせてある（`Command` と同じ規則）。ここを間違えると
/// 空白を含むパスが 2 つの引数に割れるので、テストで固めてある。
pub(crate) fn join_args(args: &[String]) -> String {
    let mut out = String::new();

    for arg in args {
        if !out.is_empty() {
            out.push(' ');
        }

        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            out.push_str(arg);
            continue;
        }

        out.push('"');
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => {
                    backslashes += 1;
                    out.push('\\');
                }
                '"' => {
                    // 引用符の前のバックスラッシュは 2 倍にしてから `\"` にする
                    for _ in 0..=backslashes {
                        out.push('\\');
                    }
                    backslashes = 0;
                    out.push('"');
                }
                _ => {
                    backslashes = 0;
                    out.push(c);
                }
            }
        }
        // 閉じ引用符の前のバックスラッシュも 2 倍にする
        for _ in 0..backslashes {
            out.push('\\');
        }
        out.push('"');
    }

    out
}

/// 起動に失敗したときのエラーダイアログを出す
///
/// 個別実行では対象の数だけ失敗が並ぶが、実行ファイルも作業フォルダも同じなので
/// 理由も同じになる。ダイアログを何枚も出さず、件数だけ添えて 1 枚にまとめる。
pub(crate) fn show_spawn_error(exe_path: &Path, reasons: &[String]) {
    let Some(reason) = reasons.first() else {
        return;
    };

    let mut message = format!(
        "起動できませんでした:\n{}\n\n{}",
        exe_path.display(),
        reason
    );

    if reasons.len() > 1 {
        message.push_str(&format!("\n\n{} 件が起動できませんでした。", reasons.len()));
    }

    if needs_interpreter(exe_path) {
        message.push_str(&format!("\n\n{}。", INTERPRETER_HINT));
    }

    show_error_dialog("エラー", &message);
}

/// スクリプトを直接指定していたときの案内
///
/// 実行時のエラーダイアログと `--check` の警告で同じ文言を使う。句点は
/// 付けない（`--check` は末尾にパスを続けるため）。
pub const INTERPRETER_HINT: &str =
    "スクリプトは直接起動できません。powershell.exe -File や wscript.exe を経由してください";

/// `CreateProcess` が実行ファイルとして起動できない拡張子か
///
/// `Command::spawn` は `CreateProcess` なので、関連付けを見て起動する
/// スクリプトは直接扱えない（それは `ShellExecute` の仕事）。
///
/// **`.bat` と `.cmd` はこの表に入れない。** 標準ライブラリがこの 2 つだけを
/// 特別扱いし、`CreateProcess` を呼ぶ前にプログラムを `cmd.exe /c` に
/// 差し替えるため、実際には起動できる（`std::sys::process::windows` の
/// `is_batch_file`）。ここに入れると、正しく動いている設定に `--check` が
/// 誤った警告を出す。
///
/// `.ps1` を同じように自動で `powershell.exe` に差し替えることはしない。
/// 実行ポリシーの既定が `Restricted` で、迂回するには `-ExecutionPolicy
/// Bypass` を黙って付けるほかなく（しかもグループポリシーは迂回できない）、
/// `powershell.exe` と `pwsh.exe` のどちらを指すかもウィンドウの出し方も
/// 決められないため。ExtRun が代われない選択は利用者に残す。
///
/// 区切りを含まない名前か（`CreateProcess` が PATH から探す）
///
/// `notepad.exe` のように書かれた実行ファイルは、`Command` に渡せば PATH から
/// 見つかる。**起動する前に存在を確かめてはいけない** — `Path::exists()` は
/// カレントフォルダを基準に解決するので、PATH にあるものを「見つかりません」と
/// 撥ねてしまう。`--check` が相対パスの存在を確認しないのと同じ線引き。
///
/// 見つからなければ `spawn()` が失敗し、いつもの起動失敗ダイアログが出る。
pub(crate) fn searched_on_path(exe_path: &Path) -> bool {
    // `is_none_or` は Rust 1.82 から。`rust-version` は 1.77.2 に留めてある
    match exe_path.parent() {
        Some(parent) => parent.as_os_str().is_empty(),
        None => true,
    }
}

/// 実行時の失敗ダイアログと `--check` の警告が同じ判断をするよう、表はここ
/// 1 か所に置く。
pub fn needs_interpreter(exe_path: &Path) -> bool {
    let Some(extension) = exe_path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(extension.to_lowercase().as_str(), "ps1" | "vbs" | "js")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 区切りを含まない名前は `CreateProcess` が PATH から探す。起動する前に
    /// 存在を確かめると、カレントフォルダ基準で解決して撥ねてしまう
    #[test]
    fn 区切りのない名前は_path_から探す() {
        assert!(searched_on_path(Path::new("notepad.exe")));
        assert!(!searched_on_path(Path::new("tools\\app.exe")));
        assert!(!searched_on_path(Path::new("C:\\Windows\\notepad.exe")));
    }

    /// `ShellExecuteExW` に渡す 1 本のコマンドラインの組み立て
    ///
    /// 受け取り側の `CommandLineToArgvW` が元の並びに戻せる形でなければならない。
    /// ここを間違えると、空白を含むパスが 2 つの引数に割れる。
    #[test]
    fn 引数を1本のコマンドラインにする() {
        // 引用符が要らないものはそのまま
        assert_eq!(
            join_args(&["-n".into(), "C:\\a\\b.txt".into()]),
            "-n C:\\a\\b.txt"
        );

        // 空白を含むものだけ囲む
        assert_eq!(
            join_args(&["-n".into(), "C:\\a b\\c.txt".into()]),
            "-n \"C:\\a b\\c.txt\""
        );

        // 引用符は \" にする
        assert_eq!(join_args(&["say \"hi\"".into()]), "\"say \\\"hi\\\"\"");

        // 閉じ引用符の直前のバックスラッシュは 2 倍にする（`C:\a b\` が壊れないように）
        assert_eq!(join_args(&["C:\\a b\\".into()]), "\"C:\\a b\\\\\"");

        // 空の引数は `""` として残す（省略すると引数の数が変わる）
        assert_eq!(join_args(&["-x".into(), String::new()]), "-x \"\"");

        assert_eq!(join_args(&[]), "");
    }

    // -----------------------------------------------------------------
    // 起動の間隔（:delay）と順番（:wait）

    /// 短い待ちのためにダイアログが一瞬出て消えるのは煩わしいので、
    /// 合計がしきい値に届いたときだけ出す
    #[test]
    fn 進行状況を出すかどうかは待ち時間の合計で決まる() {
        assert!(!shows_progress(false, 0, 20), "間隔が無ければ出さない");
        assert!(!shows_progress(false, 500, 1), "1 つだけなら待ちが無い");
        assert!(
            !shows_progress(false, 300, 3),
            "合計 600 ミリ秒では出さない"
        );
        assert!(shows_progress(false, 500, 3), "合計 1 秒で出す");
        assert!(shows_progress(false, 300, 20), "合計 5.7 秒なら出す");
    }

    /// 待つのは起動と起動の「あいだ」なので、待ちの数は対象の数より 1 少ない
    #[test]
    fn しきい値はちょうど一秒から() {
        assert!(!shows_progress(false, 999, 2), "999 ミリ秒では出さない");
        assert!(shows_progress(false, 1000, 2), "1000 ミリ秒で出す");
    }

    /// `:wait` は待ち時間が事前に決まらないので、しきい値では判断できない。
    /// 中止も一時停止もできないまま待たされる状態を作らないよう、2 件以上なら
    /// 必ず出す
    #[test]
    fn 終了を待つときは必ず進行状況を出す() {
        assert!(shows_progress(true, 0, 2), ":delay が無くても出す");
        assert!(shows_progress(true, 0, 100), "件数が多くても同じ");
        assert!(
            !shows_progress(true, 0, 1),
            "1 つだけなら待つ相手がいないので出さない"
        );
    }

    fn 対象(paths: &[&str]) -> Vec<Target> {
        paths
            .iter()
            .map(|path| Target {
                file_type: ".txt".to_string(),
                path: PathBuf::from(path),
            })
            .collect()
    }

    fn 起動(count: usize) -> Vec<Invocation> {
        (0..count)
            .map(|_| Invocation {
                program: PathBuf::from("C:\\a.exe"),
                args: Vec::new(),
                working_dir: String::new(),
                admin: false,
            })
            .collect()
    }

    #[test]
    fn 残りは起動を試みたところから後ろ() {
        let targets = 対象(&["C:\\1.txt", "C:\\2.txt", "C:\\3.txt"]);
        let invocations = 起動(3);

        assert_eq!(
            remaining_paths(&targets, &invocations, 1),
            vec!["C:\\2.txt".to_string(), "C:\\3.txt".to_string()],
            "1 つ起動したなら残りは 2 つ"
        );
        assert!(
            remaining_paths(&targets, &invocations, 3).is_empty(),
            "最後まで行けば残らない"
        );
    }

    /// `+`（まとめて渡す）は 1 プロセスなので、対象と起動の数が食い違う。
    /// 番号で対応づけられないので、残りは数えない
    #[test]
    fn まとめて渡すときは残りを数えない() {
        let targets = 対象(&["C:\\1.txt", "C:\\2.txt", "C:\\3.txt"]);
        assert!(remaining_paths(&targets, &起動(1), 0).is_empty());
    }

    /// `:wait` の要は「終わったかどうか」を見分けられること。実際にプロセスを
    /// 起動して、動いている → 終わった の移り変わりを確かめる。
    ///
    /// `:admin` の経路（`ShellExecuteExW` のハンドル）は UAC が出るので
    /// 自動では確かめられない。見ているのは通常の起動の方だけ。
    #[test]
    fn 起動したプロセスの終了を見分けられる() {
        // 1 秒ほど動き続けるプロセス。すぐ終わるものだと「動いている」側を
        // 一度も観測できず、テストが素通りする
        let args = ["/c", "ping", "-n", "2", "127.0.0.1"].map(String::from);
        let mut running = match spawn_command(
            Path::new("C:\\Windows\\System32\\cmd.exe"),
            &args,
            "C:\\Windows\\System32",
            false,
            true,
        ) {
            Ok(Launch::Started(Some(running))) => running,
            Ok(Launch::Started(None)) => panic!("keep を立てたのに持ち手が返らない"),
            Ok(Launch::Cancelled) => panic!("昇格していないのに取り消される"),
            Err(reason) => panic!("起動できない: {}", reason),
        };

        assert!(running.alive(), "起動した直後はまだ動いている");

        let 期限 = std::time::Instant::now() + Duration::from_secs(20);
        while running.alive() && std::time::Instant::now() < 期限 {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!running.alive(), "終了したら動いていないと分かる");
    }

    /// 持ち手を求めなければ持ち帰らない（`:wait` を書いていない項目）
    #[test]
    fn 終了を待たないときは持ち手を残さない() {
        let args = ["/c", "exit"].map(String::from);
        match spawn_command(
            Path::new("C:\\Windows\\System32\\cmd.exe"),
            &args,
            "C:\\Windows\\System32",
            false,
            false,
        ) {
            Ok(Launch::Started(handle)) => assert!(handle.is_none()),
            other => panic!("起動できない: {:?}", other.err()),
        }
    }

    /// 進行状況ダイアログを出さない経路でも、`Busy` のあいだは番号が進まない
    /// （ダイアログを組み立てられなかったときの逃げ道）
    #[test]
    fn 順に起動するときも_busy_のあいだは進まない() {
        let mut 問い合わせ = 0;
        let mut 起動した = Vec::new();

        launch_in_order(3, 0, &mut |index| {
            問い合わせ += 1;
            // 3 回に 1 回だけ起動できる
            if 問い合わせ % 3 != 0 {
                return progress::Step::Busy;
            }
            起動した.push(index);
            progress::Step::Started
        });

        assert_eq!(起動した, vec![0, 1, 2], "順番どおりに 1 度ずつ起動する");
        assert_eq!(問い合わせ, 9, "Busy のぶんだけ問い合わせが増える");
    }

    /// `Stop`（UAC を断られた）のあとは残りを起動しない
    #[test]
    fn 打ち切られたら残りを起動しない() {
        let mut 起動した = Vec::new();

        launch_in_order(5, 0, &mut |index| {
            if index >= 2 {
                return progress::Step::Stop;
            }
            起動した.push(index);
            progress::Step::Started
        });

        assert_eq!(起動した, vec![0, 1]);
    }

    /// 進行状況ダイアログを通した `:wait` を、実際のプロセスで通しで確かめる
    ///
    /// 単体では「終了を見分けられる」ことと「Busy では進まない」ことを別々に
    /// 見ているが、その 2 つが繋がっているかはここでしか分からない。
    /// **かかった時間で判定する** — 逐次でなければ 3 つがほぼ同時に走るので、
    /// 1 つぶんの時間で終わってしまう。
    ///
    /// 進行状況ダイアログが出るため画面が要る。
    /// **`cargo test -- --ignored --test-threads=1` で実行する。**
    #[test]
    #[ignore = "画面が必要（cargo test -- --ignored で実行）"]
    fn 終了を待って順に起動する() {
        // 1 つあたり約 1 秒かかるプロセス
        let invocations: Vec<Invocation> = (0..3)
            .map(|_| Invocation {
                program: PathBuf::from("C:\\Windows\\System32\\cmd.exe"),
                args: ["/c", "ping", "-n", "2", "127.0.0.1"]
                    .map(String::from)
                    .to_vec(),
                working_dir: "C:\\Windows\\System32".to_string(),
                admin: false,
            })
            .collect();

        let 開始 = std::time::Instant::now();
        let run = launch_all("テスト", &invocations, 0, true);
        let かかった時間 = 開始.elapsed();

        assert_eq!(run.started, 3, "3 つとも起動される");
        assert!(run.interrupted.is_none(), "最後まで進む");
        // `ping -n 2` は 1 つ約 0.75 秒。重ねて起動していればその 1 つぶんで
        // 終わるので、2 秒あれば取り違えようがない
        assert!(
            かかった時間 >= Duration::from_secs(2),
            "重ねて起動していない（{:?}）",
            かかった時間
        );
    }
}
