/*!
ExtRun - ファイルやフォルダを素早く適切なアプリケーションで開くためのランチャーツール

コマンドラインから渡されたパスに応じて、extrun-config.txt に書かれたメニューを
ポップアップ表示し、選択されたコマンドを起動して終了する。
*/

mod check;
mod config;
mod console;
mod datetime;
mod dialog;
mod icon;
mod menu;
mod placeholder;
mod preview;
mod progress;
mod prompt;

use config::{Config, MenuPosition};
use std::env;
use std::path::PathBuf;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// 表示するエラー行数の上限（設定が壊れているときにダイアログが巨大化しないように）
const MAX_DIALOG_ERRORS: usize = 20;

/// 使い方（`--help` とパスを渡さずに起動したときのダイアログで使う）
const USAGE: &str = "\
使い方:
  extrun.exe <パス> [パス ...]   対象に合わせたメニューを表示する
  extrun.exe --check             設定ファイルを検証する（エラーがあれば終了コード 1）
  extrun.exe --version           バージョンを表示する
  extrun.exe --help              このヘルプを表示する

  extrun.exe --preview <パス> [パス ...]
                     メニューに出る項目と、実際に起動されるコマンドラインを
                     起動せずに表示する

オプション（設定ファイルの [extrun] より優先されます）:
  --at <位置>        メニューを表示する位置
                       cursor  マウスカーソルの位置（既定）
                       window  前面ウィンドウの中央
                       screen  画面の中央
                       X,Y     画面座標を直接指定
  --select-first     先頭の項目を選択した状態で開く
  --no-select-first  選択しない状態で開く

どのモードでも使えるオプション:
  --config <パス>    読み込む設定ファイルを指定する
                     （相対パスはカレントディレクトリ基準）

設定ファイルは、既定では extrun.exe と同じフォルダの extrun-config.txt です。";

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

/// コマンドラインで指定された、設定ファイルへの上書き
///
/// `None` は「指定なし」。`Some` のときだけ設定ファイルの値を置き換えるので、
/// 設定で `select-first = yes` にしていても `--no-select-first` で打ち消せる。
#[derive(Debug, Default, PartialEq)]
struct Overrides {
    menu_position: Option<MenuPosition>,
    select_first: Option<bool>,
}

impl Overrides {
    /// 設定ファイルの内容に重ねる
    fn apply(&self, settings: &mut config::Settings) {
        if let Some(position) = self.menu_position {
            settings.menu_position = position;
        }
        if let Some(select_first) = self.select_first {
            settings.select_first = select_first;
        }
    }
}

/// 引数からオプションを取り出し、残りをパスとして返す
///
/// 表示位置と初期選択は「設定ファイルの属性」ではなく「呼び出し方の属性」なので
/// 引数でも指定できる。右クリック登録とホットキーで同じ設定ファイルを使いながら、
/// 出す位置を変えられる。
fn take_options(args: Vec<String>) -> Result<(Overrides, Vec<String>), String> {
    let mut overrides = Overrides::default();
    let mut paths = Vec::with_capacity(args.len());
    let mut rest = args.into_iter();

    while let Some(arg) = rest.next() {
        // `--at 値` と `--at=値` の両方を受ける
        let at = match arg.strip_prefix("--at") {
            Some("") => Some(rest.next().ok_or_else(|| {
                "--at には位置を指定してください（cursor / window / screen / X,Y）".to_string()
            })?),
            // `--attic` のような別の語を巻き込まないよう、`=` があるときだけ値と見なす
            Some(value) => value.strip_prefix('=').map(|value| value.to_string()),
            None => None,
        };

        if let Some(value) = at {
            let position = config::parse_menu_position(&value).ok_or_else(|| {
                format!(
                    "--at の位置が不正です（cursor / window / screen / X,Y）:\n{}",
                    value
                )
            })?;
            overrides.menu_position = Some(position);
            continue;
        }

        match arg.as_str() {
            "--select-first" => overrides.select_first = Some(true),
            "--no-select-first" => overrides.select_first = Some(false),
            _ => paths.push(arg),
        }
    }

    Ok((overrides, paths))
}

/// `--config <パス>` を取り出し、残りの引数を返す
///
/// `--check` は `take_options` より前で処理されるので、設定ファイルの指定も
/// そこより前に切り出す必要がある。`take_options` ごと前に動かさないのは、
/// `--at` の書き間違いで `--version` や `--help` まで出せなくなるため。
fn take_config_path(args: Vec<String>) -> Result<(Option<PathBuf>, Vec<String>), String> {
    const MISSING_VALUE: &str = "--config には設定ファイルのパスを指定してください";

    let mut config = None;
    let mut rest = Vec::with_capacity(args.len());
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        // `--config 値` と `--config=値` の両方を受ける
        let value = match arg.strip_prefix("--config") {
            Some("") => Some(args.next().ok_or_else(|| MISSING_VALUE.to_string())?),
            // 別の語を巻き込まないよう、`=` があるときだけ値と見なす
            Some(value) => value.strip_prefix('=').map(|value| value.to_string()),
            None => None,
        };

        match value {
            Some(value) if value.is_empty() => return Err(MISSING_VALUE.to_string()),
            Some(value) => config = Some(PathBuf::from(value)),
            None => rest.push(arg),
        }
    }

    Ok((config, rest))
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

/// 設定ファイルのパス
///
/// 既定は実行ファイルと同じフォルダ。`--config` で指定されたときは**カレント
/// ディレクトリ基準**で解決する（`cd` した先から `--config test\a.txt` と
/// 打てるほうが、テスト用の設定を切り替える用途に合う）。
fn config_path(override_path: Option<PathBuf>) -> Option<PathBuf> {
    match override_path {
        Some(path) => Some(to_absolute_path(&path)),
        None => Some(
            env::current_exe()
                .ok()?
                .parent()?
                .join(config::CONFIG_FILE_NAME),
        ),
    }
}

/// プロセスをモニタごとの DPI に対応させる
///
/// 宣言しないと Windows が 96 DPI で描いた結果を引き伸ばすので、高 DPI の画面では
/// メニューの文字がぼやける。座標も仮想化され、`GetCursorPos` などが物理ピクセルを
/// 返さなくなる。
///
/// V2 でないといけないのは、**メニューと非クライアント領域の自動スケーリングが
/// V2 で追加された**ため。このアプリの UI はメニューがすべてなので V1 では効かない。
/// ウィンドウを作る前に呼ぶ必要があるので `main()` の先頭に置く。
///
/// マニフェストで宣言するのが Microsoft の推奨だが、`build.rs` が `.rc` を
/// `OUT_DIR` に生成する構成なのでマニフェストも生成物になり、手数が増える。
/// API なら 1 行で済み、`windows-sys` のフィーチャーが 1 つ増えるだけで
/// 依存クレートは増えない。Windows 10 1703 以降が必要だが、同梱サンプルが
/// `tar.exe` を使っている時点で下限はすでに 1803 なので実質の制約にならない。
fn enable_dpi_awareness() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn main() {
    enable_dpi_awareness();

    let args: Vec<String> = env::args().skip(1).collect();

    // 設定ファイルの指定は `--check` より前に切り出す（`--check --config x` が効くように）。
    // エラーの出し先は、コンソールに出す 2 つのモードかどうかで決める
    let to_console = args
        .iter()
        .any(|arg| arg == "--preview" || arg == "--check");
    let (config_override, args) = match take_config_path(args) {
        Ok(result) => result,
        Err(message) => {
            report_usage_error(to_console, &format!("{}\n\n{}", message, USAGE));
            std::process::exit(1);
        }
    };

    let Some(config_path) = config_path(config_override) else {
        show_error_dialog("エラー", "実行ファイルの場所を取得できません。");
        std::process::exit(1);
    };

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

    // --preview は対象のパスが要るので、--check のようにここでは処理しきれない。
    // 印だけ取って残りは通常どおり進め、対象が揃ったところで分岐する。
    let preview = args.iter().any(|arg| arg == "--preview");
    let args: Vec<String> = args.into_iter().filter(|arg| arg != "--preview").collect();

    // 設定ファイルへの上書きを取り出し、残りをパスとして扱う
    let (overrides, args) = match take_options(args) {
        Ok(result) => result,
        Err(message) => {
            report_usage_error(preview, &format!("{}\n\n{}", message, USAGE));
            std::process::exit(1);
        }
    };

    if args.is_empty() {
        report_usage_error(
            preview,
            &format!("ファイルパスが指定されていません。\n\n{}", USAGE),
        );
        std::process::exit(1);
    }

    // ターゲットの設定（存在しないパスは対象にできないので落とす）
    let (targets, missing) = build_targets(&args);

    // 1 つでも開けるものがあればメニューを出す。全部だめなときだけ理由を見せる
    if targets.is_empty() {
        report_usage_error(
            preview,
            &format!(
                "指定されたパスが見つかりません:\n{}",
                format_missing(&missing)
            ),
        );
        std::process::exit(1);
    }

    if preview {
        std::process::exit(preview::run(&config_path, &targets));
    }

    // 設定ファイルの読み込み
    let mut parsed = match Config::load(&config_path) {
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

    // 引数の指定は設定ファイルより優先する
    overrides.apply(&mut parsed.config.settings);

    // メニューを作成して表示
    menu::create_and_show_menu(&parsed.config, &targets);
}

/// 引数のパスから対象を作る（存在しないものは第 2 の戻り値に分ける）
fn build_targets(args: &[String]) -> (Vec<Target>, Vec<&str>) {
    let mut targets: Vec<Target> = Vec::with_capacity(args.len());
    let mut missing: Vec<&str> = Vec::new();

    for arg in args {
        let path = PathBuf::from(arg);
        if path.exists() {
            targets.push(Target::from_path(to_absolute_path(&path)));
        } else {
            missing.push(arg);
        }
    }

    (targets, missing)
}

/// 呼び出し方の誤りを伝える
///
/// `--preview` はコンソールで使うものなので、ダイアログを出されると
/// リダイレクトしていた場合に何も残らない。出力先を揃える。
fn report_usage_error(preview: bool, message: &str) {
    if preview {
        console::print(&format!("{}\r\n", message.replace('\n', "\r\n")));
    } else {
        show_error_dialog("ExtRun", message);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn take(args: &[&str]) -> Result<(Overrides, Vec<String>), String> {
        take_options(args.iter().map(|s| s.to_string()).collect())
    }

    fn take_config(args: &[&str]) -> Result<(Option<PathBuf>, Vec<String>), String> {
        take_config_path(args.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn 設定ファイルの指定を取り除いてパスだけ返す() {
        let (config, rest) = take_config(&["--config", "test.txt", "a.txt"]).unwrap();
        assert_eq!(config, Some(PathBuf::from("test.txt")));
        assert_eq!(rest, vec!["a.txt"]);
    }

    #[test]
    fn イコール付きの設定ファイル指定も読める() {
        let (config, rest) = take_config(&["--config=D:\\t\\c.txt", "a.txt"]).unwrap();
        assert_eq!(config, Some(PathBuf::from("D:\\t\\c.txt")));
        assert_eq!(rest, vec!["a.txt"]);
    }

    /// `--check` は `take_options` より前で処理されるので、そこでも効く必要がある
    #[test]
    fn 検証モードと一緒でも設定ファイルを指定できる() {
        let (config, rest) = take_config(&["--check", "--config", "test.txt"]).unwrap();
        assert_eq!(config, Some(PathBuf::from("test.txt")));
        assert_eq!(rest, vec!["--check"]);
    }

    #[test]
    fn 指定がなければ設定ファイルは既定のまま() {
        let (config, rest) = take_config(&["a.txt", "b.txt"]).unwrap();
        assert_eq!(config, None);
        assert_eq!(rest, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn 値の無い設定ファイル指定はエラー() {
        assert!(take_config(&["--config"]).is_err());
        assert!(take_config(&["--config=", "a.txt"]).is_err());
    }

    #[test]
    fn 前方一致する別の語は設定ファイル指定にしない() {
        let (config, rest) = take_config(&["--configure", "a.txt"]).unwrap();
        assert_eq!(config, None);
        assert_eq!(rest, vec!["--configure", "a.txt"]);
    }

    #[test]
    fn オプションを取り除いてパスだけ返す() {
        let (overrides, paths) = take(&["--at", "window", "a.txt", "b.txt"]).unwrap();
        assert_eq!(overrides.menu_position, Some(MenuPosition::Window));
        assert_eq!(paths, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn イコール付きのオプションも読める() {
        let (overrides, paths) = take(&["--at=100,200", "a.txt"]).unwrap();
        assert_eq!(
            overrides.menu_position,
            Some(MenuPosition::Point { x: 100, y: 200 })
        );
        assert_eq!(paths, vec!["a.txt"]);
    }

    #[test]
    fn 初期選択の指定を読める() {
        let (overrides, _) = take(&["--select-first", "a.txt"]).unwrap();
        assert_eq!(overrides.select_first, Some(true));
    }

    /// 設定ファイルで yes にしていても打ち消せる必要がある
    #[test]
    fn 初期選択の打ち消しが効く() {
        let (overrides, _) = take(&["--no-select-first", "a.txt"]).unwrap();
        assert_eq!(overrides.select_first, Some(false));
    }

    #[test]
    fn オプションを指定しなければ上書きしない() {
        let (overrides, paths) = take(&["a.txt"]).unwrap();
        assert_eq!(overrides, Overrides::default());
        assert_eq!(overrides.menu_position, None);
        assert_eq!(overrides.select_first, None);
        assert_eq!(paths, vec!["a.txt"]);
    }

    #[test]
    fn 値の無いオプションはエラー() {
        let error = take(&["--at"]).unwrap_err();
        assert!(error.contains("位置を指定してください"), "{}", error);
    }

    #[test]
    fn 不正な位置はエラー() {
        let error = take(&["--at", "middle", "a.txt"]).unwrap_err();
        assert!(error.contains("位置が不正です"), "{}", error);
    }

    /// `--at` で始まる別の語をオプションと取り違えない
    #[test]
    fn 前方一致する別の語はパスとして扱う() {
        let (overrides, paths) = take(&["--attic", "a.txt"]).unwrap();
        assert_eq!(overrides.menu_position, None);
        assert_eq!(paths, vec!["--attic", "a.txt"]);
    }

    #[test]
    fn 上書きは設定ファイルに重なる() {
        let mut settings = config::Settings::default();
        let (overrides, _) = take(&["--at", "screen", "a.txt"]).unwrap();
        overrides.apply(&mut settings);

        assert_eq!(settings.menu_position, MenuPosition::Screen);
        // 指定していない項目は設定ファイルの値のまま
        assert!(!settings.select_first);
    }
}
