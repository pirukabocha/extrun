/*!
メニューの作成と表示 (Win32 API版)
*/

use crate::config::{Config, MenuItem};
use crate::placeholder::PathPlaceholders;
use crate::Target;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// メニューアクション
#[derive(Clone)]
enum MenuAction {
    ExecuteApp {
        item: MenuItem,
        targets: Arc<Vec<Target>>,
    },
    Close,
}

/// グローバル状態（メニューIDとアクションのマッピング）
struct GlobalState {
    actions: HashMap<u16, MenuAction>,
    next_id: u16,
}

impl GlobalState {
    fn new() -> Self {
        GlobalState {
            actions: HashMap::new(),
            next_id: 1000, // WM_COMMANDで使用する開始ID
        }
    }

    fn add_action(&mut self, action: MenuAction) -> u16 {
        let id = self.next_id;
        self.actions.insert(id, action);
        self.next_id += 1;
        id
    }
}

/// メニューを作成して表示
pub fn create_and_show_menu(config: &Config, targets: &[Target]) {
    // グローバル状態を作成
    let state = Arc::new(Mutex::new(GlobalState::new()));
    let shared_targets = Arc::new(targets.to_vec());

    // メニュー項目をフィルタリング
    let filtered_apps = filter_menu_items(&config.apps, targets);

    if filtered_apps.is_empty() {
        show_error_dialog(
            "情報",
            "対象となるファイルに適用できるメニュー項目がありません。",
        );
        return;
    }

    // ウィンドウクラスの登録
    let class_name = to_wide_string("ExtRunMenuClass");
    let window_title = to_wide_string("ExtRun");
    let h_instance = unsafe { GetModuleHandleW(null_mut()) };

    let wnd_class = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: h_instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: null_mut(),
        lpszMenuName: null_mut(),
        lpszClassName: class_name.as_ptr(),
    };

    unsafe { RegisterClassW(&wnd_class) };

    // 非表示のウィンドウを作成
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            null_mut(),
            null_mut(),
            h_instance,
            null_mut(),
        )
    };

    if hwnd.is_null() {
        show_error_dialog("エラー", "ウィンドウの作成に失敗しました。");
        return;
    }

    // ポップアップメニューを作成
    let hmenu = unsafe { CreatePopupMenu() };

    // アプリケーションメニューを追加
    add_menu_items(hmenu, &filtered_apps, Arc::clone(&shared_targets), &state);

    // 閉じるメニューを追加
    append_separator(hmenu);
    let close_id = state.lock().unwrap().add_action(MenuAction::Close);
    append_menu_item(hmenu, close_id, "閉じる", MF_STRING);

    // カーソル位置を取得
    let mut cursor_pos = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut cursor_pos) };

    // ウィンドウをフォアグラウンドに設定（メニュー表示のため必要）
    unsafe { SetForegroundWindow(hwnd) };

    // ポップアップメニューを表示
    let cmd = unsafe {
        TrackPopupMenu(
            hmenu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            cursor_pos.x,
            cursor_pos.y,
            0,
            hwnd,
            null_mut(),
        )
    };

    // メニューを破棄
    unsafe { DestroyMenu(hmenu) };

    // コマンドを実行
    if cmd != 0 {
        if let Some(action) = state.lock().unwrap().actions.remove(&(cmd as u16)) {
            execute_action(action);
        }
    }

    // ウィンドウを破棄
    unsafe { DestroyWindow(hwnd) };
}

/// メニュー項目を追加
fn add_menu_items(
    hmenu: HMENU,
    items: &[MenuItem],
    targets: Arc<Vec<Target>>,
    state: &Arc<Mutex<GlobalState>>,
) {
    for item in items {
        if item.is_separator() {
            append_separator(hmenu);
        } else if item.has_submenu() {
            let submenu = unsafe { CreatePopupMenu() };
            add_menu_items(submenu, &item.submenu, Arc::clone(&targets), state);
            append_submenu(hmenu, submenu, &item.name);
        } else {
            let id = state.lock().unwrap().add_action(MenuAction::ExecuteApp {
                item: item.clone(),
                targets: Arc::clone(&targets),
            });
            append_menu_item(hmenu, id, &item.name, MF_STRING);
        }
    }
}

/// メニュー項目を追加（ヘルパー関数）
fn append_menu_item(hmenu: HMENU, id: u16, text: &str, flags: u32) {
    let text_wide = to_wide_string(text);
    unsafe { AppendMenuW(hmenu, flags, id as usize, text_wide.as_ptr()) };
}

/// サブメニューを追加（ヘルパー関数）
fn append_submenu(hmenu: HMENU, submenu: HMENU, text: &str) {
    let text_wide = to_wide_string(text);
    unsafe { AppendMenuW(hmenu, MF_POPUP, submenu as usize, text_wide.as_ptr()) };
}

/// セパレーターを追加（ヘルパー関数）
fn append_separator(hmenu: HMENU) {
    unsafe { AppendMenuW(hmenu, MF_SEPARATOR, 0, null_mut()) };
}

/// 文字列をワイド文字列に変換
fn to_wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

/// ウィンドウプロシージャ
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// メニュー項目をフィルタリング
pub fn filter_menu_items(apps: &[MenuItem], targets: &[Target]) -> Vec<MenuItem> {
    let target_info = TargetInfo::from_targets(targets);
    if target_info.file_types.is_empty() {
        return Vec::new();
    }
    filter_with_info(apps, &target_info)
}

/// ターゲット判定用の前処理情報
struct TargetInfo {
    has_folder: bool,
    has_non_folder: bool,
    file_types: HashSet<String>,
}

impl TargetInfo {
    fn from_targets(targets: &[Target]) -> Self {
        let mut has_folder = false;
        let mut has_non_folder = false;
        let mut file_types = HashSet::with_capacity(targets.len());

        for target in targets {
            if target.file_type == "folder" {
                has_folder = true;
            } else {
                has_non_folder = true;
            }
            file_types.insert(target.file_type.clone());
        }

        TargetInfo {
            has_folder,
            has_non_folder,
            file_types,
        }
    }
}

/// 対象に合う項目だけを残す（拡張子はパース時に解決済み）
fn filter_with_info(apps: &[MenuItem], target_info: &TargetInfo) -> Vec<MenuItem> {
    let mut menu_items = Vec::with_capacity(apps.len());

    for app in apps {
        if app.has_submenu() {
            // 子が 1 つも残らなかったサブメニューは丸ごと落とす
            let filtered_submenu = filter_with_info(&app.submenu, target_info);
            if !filtered_submenu.is_empty() {
                let mut new_app = app.clone();
                new_app.submenu = filtered_submenu;
                menu_items.push(new_app);
            }
        } else if is_menu_item_applicable(&app.extensions, target_info) {
            menu_items.push(app.clone());
        }
    }

    cleanup_separators(menu_items)
}

/// メニュー項目が対象に適用可能か判定
fn is_menu_item_applicable(extensions: &[String], target_info: &TargetInfo) -> bool {
    if extensions.is_empty() {
        return true;
    }

    if target_info.has_non_folder && extensions.iter().any(|ext| ext == "file") {
        return true;
    }

    if target_info.has_folder && extensions.iter().any(|ext| ext == "folder") {
        return true;
    }

    extensions
        .iter()
        .any(|ext| target_info.file_types.contains(ext))
}

/// セパレーターをクリーンアップ
fn cleanup_separators(items: Vec<MenuItem>) -> Vec<MenuItem> {
    // 先頭のセパレーターをスキップ
    let first_non_separator = items
        .iter()
        .position(|item| !item.is_separator())
        .unwrap_or(items.len());

    // 連続するセパレーターを1つにまとめる
    let mut filtered = Vec::with_capacity(items.len().saturating_sub(first_non_separator));
    let mut prev_separator = false;

    for item in items.into_iter().skip(first_non_separator) {
        if item.is_separator() {
            if !prev_separator {
                filtered.push(item);
                prev_separator = true;
            }
        } else {
            filtered.push(item);
            prev_separator = false;
        }
    }

    // 末尾のセパレーターを削除
    if filtered.last().is_some_and(|item| item.is_separator()) {
        filtered.pop();
    }

    filtered
}

/// アクションを実行
fn execute_action(action: MenuAction) {
    match action {
        MenuAction::ExecuteApp { item, targets } => {
            execute_command(&item, targets.as_slice());
        }
        MenuAction::Close => {
            // 何もしない
        }
    }
}

/// コマンドを実行
fn execute_command(item: &MenuItem, targets: &[Target]) {
    if targets.is_empty() {
        return;
    }

    let exe_path = PathBuf::from(&item.path);
    if !exe_path.exists() {
        show_error_dialog(
            "エラー",
            &format!("実行ファイルが見つかりません:\n{}", exe_path.display()),
        );
        return;
    }

    // working_dir の処理
    let working_dir = if !item.working_dir.is_empty() {
        let placeholders = PathPlaceholders::from_path(&targets[0].path);
        placeholders.replace(&item.working_dir)
    } else {
        exe_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string())
    };

    // コマンドを実行
    if item.all_mode {
        execute_all_mode(&exe_path, &item.args, targets, &working_dir);
    } else {
        execute_multi_mode(&exe_path, &item.args, targets, &working_dir);
    }
}

/// プロセスを起動する（失敗したら理由を返す）
fn spawn_command(exe_path: &Path, args: &[String], working_dir: &str) -> Result<(), String> {
    Command::new(exe_path)
        .args(args)
        .current_dir(working_dir)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 起動に失敗したときのエラーダイアログを出す
///
/// 個別実行では対象の数だけ失敗が並ぶが、実行ファイルも作業フォルダも同じなので
/// 理由も同じになる。ダイアログを何枚も出さず、件数だけ添えて 1 枚にまとめる。
fn show_spawn_error(exe_path: &Path, reasons: &[String]) {
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

    // CreateProcess はバッチファイルやスクリプトを直接起動できない
    let extension = exe_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());
    if let Some("bat" | "cmd" | "ps1" | "vbs" | "js") = extension.as_deref() {
        message.push_str(
            "\n\nスクリプトは直接起動できません。cmd.exe /c や powershell.exe -File を経由してください。",
        );
    }

    show_error_dialog("エラー", &message);
}

/// 全てのパスをまとめて1つのプロセスで実行
fn execute_all_mode(exe_path: &Path, base_args: &[String], targets: &[Target], working_dir: &str) {
    let placeholder_count = base_args.iter().filter(|arg| arg.as_str() == "$p").count();
    let has_path_placeholder = base_args.iter().any(|arg| arg.contains("$p"));
    let extra_path_args = if has_path_placeholder {
        placeholder_count.saturating_mul(targets.len().saturating_sub(1))
    } else {
        targets.len()
    };
    let mut final_args = Vec::with_capacity(base_args.len() + extra_path_args);
    let first_path = &targets[0].path;

    let placeholders = PathPlaceholders::from_path(first_path);
    for arg in base_args {
        if arg == "$p" {
            for target in targets {
                final_args.push(target.path.to_string_lossy().to_string());
            }
        } else {
            final_args.push(placeholders.replace(arg));
        }
    }

    if !has_path_placeholder {
        for target in targets {
            final_args.push(target.path.to_string_lossy().to_string());
        }
    }

    if let Err(reason) = spawn_command(exe_path, &final_args, working_dir) {
        show_spawn_error(exe_path, &[reason]);
    }
}

/// それぞれのパスを個別のプロセスで並列実行
fn execute_multi_mode(
    exe_path: &Path,
    base_args: &[String],
    targets: &[Target],
    working_dir: &str,
) {
    let mut reasons: Vec<String> = Vec::new();

    for target in targets {
        let placeholders = PathPlaceholders::from_path(&target.path);
        let args = placeholders.replace_args(base_args);
        if let Err(reason) = spawn_command(exe_path, &args, working_dir) {
            reasons.push(reason);
        }
    }

    show_spawn_error(exe_path, &reasons);
}

/// エラーダイアログを表示
fn show_error_dialog(title: &str, message: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

        unsafe {
            let title_wide = to_wide_string(title);
            let message_wide = to_wide_string(message);

            MessageBoxW(
                null_mut(),
                message_wide.as_ptr(),
                title_wide.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    #[cfg(not(windows))]
    {
        eprintln!("{}: {}", title, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    /// 実際の設定ファイルを読む（テスト用フィクスチャ兼サンプル）
    fn sample_config() -> Config {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/extrun-config.txt"))
                .expect("extrun-config.txt を読める");

        let parsed = parse(&text);
        let errors: Vec<String> = parsed
            .errors()
            .map(|d| format!("{}行目: {}", d.line, d.message))
            .collect();
        assert!(errors.is_empty(), "設定ファイルのエラー: {:?}", errors);
        parsed.config
    }

    fn target(file_type: &str) -> Target {
        Target {
            file_type: file_type.to_string(),
            path: PathBuf::from("C:\\dummy\\sample"),
        }
    }

    /// セパレーターとサブメニューの中身も含めた項目数
    fn count_items(items: &[MenuItem]) -> usize {
        items
            .iter()
            .map(|item| 1 + count_items(&item.submenu))
            .sum()
    }

    fn menu_for(config: &Config, file_type: &str) -> Vec<MenuItem> {
        filter_menu_items(&config.apps, &[target(file_type)])
    }

    #[test]
    fn 対象ごとの項目数が期待どおり() {
        // extrun-config.txt から構築されるメニューの項目数
        // （セパレーターとサブメニューの中身も数える）
        let expected = [
            (".png", 24),
            (".jpg", 24),
            (".gif", 27),
            (".ico", 24),
            (".bmp", 24),
            (".tif", 25),
            (".mp3", 19),
            (".wav", 19),
            (".mp4", 19),
            (".mkv", 19),
            (".zip", 19),
            (".tar", 19),
            (".gz", 19),
            (".cab", 17),
            (".txt", 19),
            (".md", 19),
            (".csv", 19),
            // どのセクションにも該当しない拡張子は [file] と [file folder] だけ
            (".pdf", 15),
            ("file", 15),
            ("folder", 20),
        ];

        let config = sample_config();
        let mut mismatches = Vec::new();

        for (file_type, count) in expected {
            let actual = count_items(&menu_for(&config, file_type));
            if actual != count {
                mismatches.push(format!("{}: 期待 {} / 実際 {}", file_type, count, actual));
            }
        }

        assert!(mismatches.is_empty(), "項目数の不一致: {:#?}", mismatches);
    }

    #[test]
    fn 先頭のセパレーターは取り除かれる() {
        // file は [file] セクションの先頭セパレーターが最初の項目になる
        let config = sample_config();
        let menu = menu_for(&config, "file");
        assert!(!menu[0].is_separator());
        assert_eq!(menu[0].name, "親フォルダを開いて選択");
        assert!(!menu.last().expect("項目がある").is_separator());
    }

    #[test]
    fn jpg_のメニュー構造() {
        let config = sample_config();
        let menu = menu_for(&config, ".jpg");
        let names: Vec<&str> = menu.iter().map(|item| item.name.as_str()).collect();

        assert_eq!(
            names,
            vec![
                "開く",
                "画像のサイズを調べる",
                "形式を変換",
                "長辺 1280px に縮小する",
                "---",
                "親フォルダを開いて選択",
                "読み取り専用・隠し属性を解除",
                "SHA256 を書き出す",
                "---",
                "サイズを調べる",
                "---",
                "圧縮",
                "---",
                "パスをコピーする",
            ]
        );

        // [-.jpg -.jpeg] と [.gif] の子は落ち、末尾に残るセパレーターも消える
        let convert = &menu[2];
        assert_eq!(convert.name, "形式を変換");
        let children: Vec<&str> = convert
            .submenu
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(children, vec!["PNG に変換", "BMP に変換"]);
    }

    #[test]
    fn folder_のサブメニューにセパレーターが残る() {
        let config = sample_config();
        let menu = menu_for(&config, "folder");
        let open = &menu[0];
        assert_eq!(open.name, "開く");
        let children: Vec<&str> = open.submenu.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            children,
            vec![
                "エクスプローラで開く",
                "---",
                "PowerShell で開く",
                "コマンドプロンプトで開く",
            ]
        );
        // 引数欄を空にした項目は引数なし、:dir はプレースホルダーを保ったまま
        assert!(open.submenu[3].args.is_empty());
        assert_eq!(open.submenu[3].working_dir, "$p");
    }

    #[test]
    fn 複数選択では和集合になる() {
        let config = sample_config();
        let menu = filter_menu_items(&config.apps, &[target(".txt"), target(".png")]);
        let names: Vec<&str> = menu.iter().map(|item| item.name.as_str()).collect();
        assert!(names.contains(&"メモ帳で開く"));
        assert!(names.contains(&"画像のサイズを調べる"));
    }

    #[test]
    fn まとめて実行の指定が読める() {
        let config = sample_config();
        let menu = menu_for(&config, "folder");
        let compress = menu
            .iter()
            .find(|item| item.name == "圧縮")
            .expect("圧縮がある");
        let zip = compress
            .submenu
            .iter()
            .find(|item| item.name == "ZIP")
            .expect("ZIP がある");
        let single = &zip.submenu[0];
        let batch = &zip.submenu[1];
        assert_eq!(single.name, "個別に圧縮");
        assert!(!single.all_mode);
        assert!(batch.all_mode);
    }

    #[test]
    fn セクションの指定は絞り込みではない() {
        // [folder] セクションの項目でも [file folder] と書けばファイルにも出る
        let config = sample_config();
        for file_type in ["folder", ".txt", ".png"] {
            let names: Vec<String> = menu_for(&config, file_type)
                .iter()
                .map(|item| item.name.clone())
                .collect();
            assert!(
                names.iter().any(|n| n == "サイズを調べる"),
                "{} に出ていない",
                file_type
            );
        }
    }
}
