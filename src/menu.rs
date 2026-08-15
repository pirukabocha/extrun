/*!
メニューの作成と表示 (Win32 API版)
*/

use crate::config::{Config, MenuItem, MenuPosition};
use crate::placeholder::{PathPlaceholders, RunContext};
use crate::Target;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_DOWN,
};
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
    // 自分が前面に出る前の前面ウィンドウ。表示位置の基準に使うので、
    // SetForegroundWindow より前に取らないと自分自身になってしまう
    let owner = unsafe { GetForegroundWindow() };

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
    // Esc でも閉じられるので、アクセスキーは予約しない
    append_menu_item(hmenu, close_id, "閉じる", None);

    // メニューを出す位置を決める
    let (point, align) = menu_anchor(config.settings.menu_position, owner);

    // ウィンドウをフォアグラウンドに設定（メニュー表示のため必要）
    unsafe { SetForegroundWindow(hwnd) };

    // 先頭項目を選択した状態で開く
    //
    // タイマーを仕掛けておくと、メニューのモーダルループが立ち上がって手が空いた
    // ところで WM_TIMER が `window_proc` に配送される。そこで初めてメニューが
    // 出ていると分かるので、キー入力を差し込む。
    //
    // TrackPopupMenu は同期的に戻るので、仕掛けるのは呼ぶ前でなければならない。
    if config.settings.select_first {
        unsafe { SetTimer(hwnd, SELECT_FIRST_TIMER, 0, None) };
    }

    // ポップアップメニューを表示
    let cmd = unsafe {
        TrackPopupMenu(
            hmenu,
            TPM_RETURNCMD | TPM_NONOTIFY | align,
            point.x,
            point.y,
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

/// メニューを出す座標と配置フラグを決める
///
/// `owner` は ExtRun が前面に出る前の前面ウィンドウ。基準にできないときは
/// 画面中央 → カーソル位置の順に落とす。
///
/// `TPM_CENTERALIGN` / `TPM_VCENTERALIGN` を付けると、渡した座標を左上ではなく
/// 中心として扱ってくれる。メニューの幅と高さを自前で測る必要はない。
/// 画面からはみ出す分も Windows が自動で寄せる。
fn menu_anchor(position: MenuPosition, owner: HWND) -> (POINT, u32) {
    const CENTERED: u32 = TPM_CENTERALIGN | TPM_VCENTERALIGN;
    // 渡した座標をメニューの左上に置く（どちらも 0 だが意図を残す）
    const TOP_LEFT: u32 = TPM_LEFTALIGN | TPM_TOPALIGN;

    match position {
        MenuPosition::Point { x, y } => (POINT { x, y }, TOP_LEFT),
        MenuPosition::Window => match window_center(owner) {
            Some(point) => (point, CENTERED),
            None => menu_anchor(MenuPosition::Screen, owner),
        },
        MenuPosition::Screen => match work_area_center(owner) {
            Some(point) => (point, CENTERED),
            None => menu_anchor(MenuPosition::Cursor, owner),
        },
        MenuPosition::Cursor => (cursor_point(), TOP_LEFT),
    }
}

/// マウスカーソルの位置
fn cursor_point() -> POINT {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    point
}

/// ウィンドウの中心
///
/// 最小化されたウィンドウの矩形は `-32000` 付近を返すので、そのまま使うと
/// 画面の外にメニューが出る。`IsIconic` で先に弾く。
fn window_center(hwnd: HWND) -> Option<POINT> {
    if hwnd.is_null() || unsafe { IsIconic(hwnd) } != 0 {
        return None;
    }

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }

    Some(rect_center(&rect))
}

/// ウィンドウがあるモニタの作業領域（タスクバーを除く）の中心
fn work_area_center(hwnd: HWND) -> Option<POINT> {
    let monitor = if hwnd.is_null() {
        unsafe { MonitorFromPoint(cursor_point(), MONITOR_DEFAULTTOPRIMARY) }
    } else {
        unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }
    };

    if monitor.is_null() {
        return None;
    }

    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }

    Some(rect_center(&info.rcWork))
}

/// 矩形の中心（幅が奇数のときは切り捨て）
fn rect_center(rect: &RECT) -> POINT {
    POINT {
        x: rect.left + (rect.right - rect.left) / 2,
        y: rect.top + (rect.bottom - rect.top) / 2,
    }
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
            append_submenu(hmenu, submenu, &item.name, item.accesskey);
        } else {
            let id = state.lock().unwrap().add_action(MenuAction::ExecuteApp {
                item: item.clone(),
                targets: Arc::clone(&targets),
            });
            append_menu_item(hmenu, id, &item.name, item.accesskey);
        }
    }
}

/// メニュー項目を追加（ヘルパー関数）
fn append_menu_item(hmenu: HMENU, id: u16, name: &str, accesskey: Option<usize>) {
    let label = to_label_wide(name, accesskey);
    unsafe { AppendMenuW(hmenu, MF_STRING, id as usize, label.as_ptr()) };
}

/// サブメニューを追加（ヘルパー関数）
fn append_submenu(hmenu: HMENU, submenu: HMENU, name: &str, accesskey: Option<usize>) {
    let label = to_label_wide(name, accesskey);
    unsafe { AppendMenuW(hmenu, MF_POPUP, submenu as usize, label.as_ptr()) };
}

/// セパレーターを追加（ヘルパー関数）
fn append_separator(hmenu: HMENU) {
    unsafe { AppendMenuW(hmenu, MF_SEPARATOR, 0, null_mut()) };
}

/// 表示名を Win32 のメニューラベルに変換する
///
/// Win32 は `&` をアクセスキーの目印として食べてしまう。表示したい `&` は
/// `&&` に二重化し、アクセスキーの位置にだけ `&` を挿し込む。
/// `MenuItem::name` は表示用の文字列なので、この変換はここでしか行わない。
fn to_label_wide(name: &str, accesskey: Option<usize>) -> Vec<u16> {
    // 大多数の項目はここで抜ける（`&` を含まず、アクセスキーも無い）
    if accesskey.is_none() && !name.contains('&') {
        return to_wide_string(name);
    }

    let mut label = String::with_capacity(name.len() + 2);
    for (i, c) in name.char_indices() {
        if Some(i) == accesskey {
            label.push('&');
        }
        label.push(c);
        if c == '&' {
            label.push('&');
        }
    }
    to_wide_string(&label)
}

/// 文字列をワイド文字列に変換
fn to_wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

/// 先頭項目を選択するために一度だけ発火させるタイマーの ID
const SELECT_FIRST_TIMER: usize = 1;

/// ウィンドウプロシージャ
///
/// ほぼ `DefWindowProcW` に委譲するだけだが、`select-first` のタイマーだけは
/// ここで受ける。このタイマーは `select-first` が有効なときしか仕掛けないので、
/// 届いた時点で「先頭を選択したい」と決まっている。
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TIMER && wparam == SELECT_FIRST_TIMER {
        KillTimer(hwnd, SELECT_FIRST_TIMER);
        send_key(VK_DOWN);
        return 0;
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// キーの押下と解放を 1 回ずつ送る
///
/// `PostMessageW` でオーナーウィンドウに投げても**メニューには届かない**。
/// メニューは自分のモーダルループで本物のキー入力を読むので、メッセージ
/// キューではなく入力そのものに差し込む必要がある。
fn send_key(key: VIRTUAL_KEY) {
    let mut input: [INPUT; 2] = unsafe { std::mem::zeroed() };

    for (index, event) in input.iter_mut().enumerate() {
        event.r#type = INPUT_KEYBOARD;
        // 共用体のフィールドは書き込むだけなら安全（読み出しは unsafe）
        event.Anonymous.ki.wVk = key;
        if index == 1 {
            event.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        }
    }

    unsafe {
        SendInput(
            input.len() as u32,
            input.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
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
        }];
    }

    targets
        .iter()
        .map(|target| Invocation {
            program: exe_path.clone(),
            args: PathPlaceholders::from_path(&target.path).replace_args(&item.args, ctx),
            working_dir: working_dir.clone(),
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

    // 日時はここで 1 回だけ確定させる。対象ごとに取り直すと、個別に起動した
    // ときに $t{ss} がずれて、まとめて作ったはずのファイル名が揃わなくなる
    let ctx = RunContext::capture();

    // 確認は項目に対して 1 回。対象の数だけ聞かれても答えは変わらない
    if !confirm_execution(item, targets, &ctx) {
        return;
    }

    // 個別実行では対象の数だけ同じ失敗が並ぶので、集めてから 1 枚だけ出す
    let mut reasons: Vec<String> = Vec::new();
    for invocation in resolve_invocations(item, targets, &ctx) {
        if let Err(reason) = spawn_command(
            &invocation.program,
            &invocation.args,
            &invocation.working_dir,
        ) {
            reasons.push(reason);
        }
    }

    show_spawn_error(&exe_path, &reasons);
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

/// 確認ダイアログの本文に並べる対象の数の上限
const MAX_CONFIRM_TARGETS: usize = 15;

/// `:confirm` が付いていれば実行前に確認する（実行してよければ `true`）
///
/// 対象の一覧を必ず添える。「何をするか」はメッセージで分かっても、「何に対して
/// するか」は選び間違えているかもしれない部分なので、目で確かめられるようにする。
fn confirm_execution(item: &MenuItem, targets: &[Target], ctx: &RunContext) -> bool {
    let Some(message) = &item.confirm else {
        return true;
    };

    let mut body = if message.is_empty() {
        format!("「{}」を実行します。", item.name)
    } else {
        // メッセージにもプレースホルダーを書ける（基準は :dir と同じく最初の対象）
        PathPlaceholders::from_path(&targets[0].path).replace(message, ctx)
    };

    body.push_str(&format!("\n\n対象: {} 件\n", targets.len()));
    for target in targets.iter().take(MAX_CONFIRM_TARGETS) {
        body.push_str(&format!("{}\n", target.path.display()));
    }
    if targets.len() > MAX_CONFIRM_TARGETS {
        body.push_str(&format!(
            "ほか {} 件\n",
            targets.len() - MAX_CONFIRM_TARGETS
        ));
    }

    body.push_str("\n実行しますか?");

    // 既定を「いいえ」にする。select-first と Enter で誤って選んだときに、
    // そのまま Enter を続けても実行されないようにするのがこの機能の主眼
    let selected = unsafe {
        MessageBoxW(
            null_mut(),
            to_wide_string(&body).as_ptr(),
            to_wide_string("ExtRun - 確認").as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };

    selected == IDYES
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
            (".png", 25),
            (".jpg", 25),
            (".gif", 28),
            (".ico", 25),
            (".bmp", 25),
            (".tif", 26),
            (".mp3", 20),
            (".wav", 20),
            (".mp4", 20),
            (".mkv", 20),
            (".zip", 20),
            (".tar", 20),
            (".gz", 20),
            (".cab", 18),
            (".txt", 20),
            (".md", 20),
            (".csv", 20),
            // どのセクションにも該当しない拡張子は [file] と [file folder] だけ
            (".pdf", 16),
            ("file", 16),
            ("folder", 21),
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
        assert_eq!(menu[0].name, "親フォルダを開いて選択 (S)");
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
                "開く (O)",
                "画像のサイズを調べる",
                "形式を変換 (C)",
                "長辺 1280px に縮小する",
                "---",
                "親フォルダを開いて選択 (S)",
                "読み取り専用・隠し属性を解除",
                "SHA256 を書き出す",
                "---",
                "サイズを調べる",
                "---",
                "圧縮 (Z)",
                "---",
                "パスをコピーする (P)",
            ]
        );

        // [-.jpg -.jpeg] と [.gif] の子は落ち、末尾に残るセパレーターも消える
        let convert = &menu[2];
        assert_eq!(convert.name, "形式を変換 (C)");
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
        assert_eq!(open.name, "開く (D)");
        let children: Vec<&str> = open.submenu.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            children,
            vec![
                "エクスプローラで開く (E)",
                "---",
                "PowerShell で開く (P)",
                "コマンドプロンプトで開く (C)",
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
        assert!(names.contains(&"メモ帳で開く (N)"));
        assert!(names.contains(&"画像のサイズを調べる"));
    }

    #[test]
    fn まとめて実行の指定が読める() {
        let config = sample_config();
        let menu = menu_for(&config, "folder");
        let compress = menu
            .iter()
            .find(|item| item.name == "圧縮 (Z)")
            .expect("圧縮がある");
        // 親が Z、子も Z。キーはメニューごとに独立しているので衝突しない
        let zip = compress
            .submenu
            .iter()
            .find(|item| item.name == "ZIP")
            .expect("ZIP がある");
        assert_eq!(compress.accesskey_char(), Some('Z'));
        assert_eq!(zip.accesskey_char(), Some('Z'));
        let single = &zip.submenu[0];
        let batch = &zip.submenu[1];
        assert_eq!(single.name, "個別に圧縮 (S)");
        assert!(!single.all_mode);
        assert!(batch.all_mode);
    }

    /// 表示名とキーの位置から、Win32 用のラベルを組み立て直せる
    #[test]
    fn アクセスキーはラベルに戻る() {
        assert_eq!(
            to_label_wide("開く (O)", Some("開く (".len())),
            to_wide_string("開く (&O)")
        );
        assert_eq!(
            to_label_wide("PNG に変換", Some(0)),
            to_wide_string("&PNG に変換")
        );
    }

    /// 表示したい `&` は Win32 に食われないように二重化する
    #[test]
    fn 名前の中のアンパサンドは二重化される() {
        assert_eq!(
            to_label_wide("Q&A のかたち", None),
            to_wide_string("Q&&A のかたち")
        );
        // アクセスキーと素の `&` は同居できる
        assert_eq!(
            to_label_wide("Q&A (X)", Some("Q&A (".len())),
            to_wide_string("Q&&A (&X)")
        );
    }

    /// `&` もアクセスキーも無い名前は組み立てを通さない（大多数の項目）
    #[test]
    fn アクセスキーのない名前はそのまま() {
        assert_eq!(
            to_label_wide("メモ帳で開く", None),
            to_wide_string("メモ帳で開く")
        );
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
