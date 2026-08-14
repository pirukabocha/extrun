/*!
ExtRun - ファイルやフォルダを素早く適切なアプリケーションで開くためのランチャーツール

コマンドラインから渡されたパスに応じて、extrun-config.txt に書かれたメニューを
ポップアップ表示し、選択されたコマンドを起動して終了する。
*/

mod check;
mod config;
mod console;
mod menu;
mod placeholder;

use config::Config;
use std::env;
use std::path::PathBuf;

/// 表示するエラー行数の上限（設定が壊れているときにダイアログが巨大化しないように）
const MAX_DIALOG_ERRORS: usize = 20;

/// 使い方（`--help` とパスを渡さずに起動したときのダイアログで使う）
const USAGE: &str = "\
使い方:
  extrun.exe <パス> [パス ...]   対象に合わせたメニューを表示する
  extrun.exe --check             設定ファイルを検証する（エラーがあれば終了コード 1）
  extrun.exe --version           バージョンを表示する
  extrun.exe --help              このヘルプを表示する

設定ファイルは extrun.exe と同じフォルダの extrun-config.txt です。";

/// ターゲットファイル/フォルダの情報
#[derive(Debug, Clone)]
pub struct Target {
    /// ファイルタイプ（`folder` / `.txt` のような拡張子 / `file`）
    pub file_type: String,
    /// ファイルパス
    pub path: PathBuf,
}

impl Target {
    /// パスからターゲットを作成
    pub fn from_path(path: PathBuf) -> Self {
        let file_type = if path.is_dir() {
            "folder".to_string()
        } else {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| format!(".{}", ext.to_lowercase()))
                .unwrap_or_else(|| "file".to_string())
        };

        Target { file_type, path }
    }
}

/// パスを絶対パスに変換（\\?\ プレフィックスなし）
fn to_absolute_path(path: &PathBuf) -> PathBuf {
    if path.is_absolute() {
        path.clone()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// 設定ファイルのパス（実行ファイルと同じフォルダ）
fn config_path() -> Option<PathBuf> {
    Some(
        env::current_exe()
            .ok()?
            .parent()?
            .join(config::CONFIG_FILE_NAME),
    )
}

fn main() {
    let Some(config_path) = config_path() else {
        show_error_dialog("エラー", "実行ファイルの場所を取得できません。");
        std::process::exit(1);
    };

    let args: Vec<String> = env::args().skip(1).collect();

    // 設定ファイルの検証
    if args.iter().any(|arg| arg == "--check") {
        std::process::exit(check::run(&config_path));
    }

    if args.iter().any(|arg| arg == "--version") {
        console::print(&format!("ExtRun {}\r\n", env!("CARGO_PKG_VERSION")));
        return;
    }

    if args
        .iter()
        .any(|arg| arg == "--help" || arg == "-h" || arg == "/?")
    {
        console::print(&format!(
            "ExtRun {}\r\n\r\n{}\r\n",
            env!("CARGO_PKG_VERSION"),
            USAGE.replace('\n', "\r\n")
        ));
        return;
    }

    if args.is_empty() {
        show_error_dialog(
            "ExtRun",
            &format!("ファイルパスが指定されていません。\n\n{}", USAGE),
        );
        std::process::exit(1);
    }

    // ターゲットの設定（存在しないパスは対象にできないので落とす）
    let mut targets: Vec<Target> = Vec::with_capacity(args.len());
    let mut missing: Vec<&str> = Vec::new();

    for arg in &args {
        let path = PathBuf::from(arg);
        if path.exists() {
            targets.push(Target::from_path(to_absolute_path(&path)));
        } else {
            missing.push(arg);
        }
    }

    // 1 つでも開けるものがあればメニューを出す。全部だめなときだけ理由を見せる
    if targets.is_empty() {
        show_error_dialog(
            "ExtRun",
            &format!(
                "指定されたパスが見つかりません:\n{}",
                format_missing(&missing)
            ),
        );
        std::process::exit(1);
    }

    // 設定ファイルの読み込み
    let parsed = match Config::load(&config_path) {
        Ok(parsed) => parsed,
        Err(message) => {
            show_error_dialog("エラー", &message);
            std::process::exit(1);
        }
    };

    if parsed.has_error() {
        show_error_dialog("設定ファイルのエラー", &format_errors(&parsed));
        std::process::exit(1);
    }

    // メニューを作成して表示
    menu::create_and_show_menu(&parsed.config, &targets);
}

/// 見つからなかったパスをダイアログ用の文字列に整形
fn format_missing(missing: &[&str]) -> String {
    let mut message = String::new();

    for (count, path) in missing.iter().enumerate() {
        if count == MAX_DIALOG_ERRORS {
            message.push_str("...\n");
            break;
        }
        message.push_str(path);
        message.push('\n');
    }

    message
}

/// パースエラーをダイアログ用の文字列に整形
fn format_errors(parsed: &config::Parsed) -> String {
    let mut message = format!("{} に問題があります。\n\n", config::CONFIG_FILE_NAME);

    for (count, diag) in parsed.errors().enumerate() {
        if count == MAX_DIALOG_ERRORS {
            message.push_str("...\n");
            break;
        }
        message.push_str(&format!("{}行目  {}\n", diag.line, diag.message));
    }

    message.push_str("\n詳しくは extrun.exe --check で確認できます。");
    message
}

/// エラーダイアログを表示
pub fn show_error_dialog(title: &str, message: &str) {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(once(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(once(0)).collect();

    unsafe {
        MessageBoxW(
            null_mut(),
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_OK | MB_ICONWARNING,
        );
    }
}
