/*!
メニューの作成と表示 (Win32 API版)
*/

use crate::config::{Config, IconMode, MenuItem, MenuPosition};
use crate::placeholder::{PathPlaceholders, RunContext};
use crate::progress;
use crate::Target;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_CANCELLED, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HBITMAP, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_DOWN,
};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// メニューアクション
///
/// `MenuItem` を箱に入れているのは、`Close` との大きさの差を詰めるため。
/// 項目の数だけ確保が増えるが、いずれにせよ複製しているので誤差の範囲。
#[derive(Clone)]
enum MenuAction {
    ExecuteApp {
        item: Box<MenuItem>,
        targets: Arc<Vec<Target>>,
        /// 起動と起動の間隔（ミリ秒。`Config::delay_of` で解決済み）
        delay: u32,
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

    // メニューを出す位置を決める
    //
    // アイコンの大きさは出す先のモニタの DPI で決まるので、項目を作るより先に
    // 位置を確定させておく
    let (point, align) = menu_anchor(config.settings.menu_position, owner);

    // ポップアップメニューを作成
    let hmenu = unsafe { CreatePopupMenu() };

    // アプリケーションメニューを追加
    let mut icons = IconCache::new(config.settings.icons, point);
    add_menu_items(
        hmenu,
        &filtered_apps,
        Arc::clone(&shared_targets),
        &state,
        &mut icons,
        config,
    );

    // 閉じるメニューを追加
    append_separator(hmenu);
    let close_id = state.lock().unwrap().add_action(MenuAction::Close);
    // Esc でも閉じられるので、アクセスキーは予約しない
    append_menu_item(hmenu, close_id, "閉じる", None);

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

    // メニューを破棄（アイコンはメニューが手放してから解放する）
    unsafe { DestroyMenu(hmenu) };
    icons.dispose();

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
    icons: &mut IconCache,
    config: &Config,
) {
    // 追加した順にそのまま並ぶので、繰り返しの番号がそのまま位置になる。
    // アイコンは位置で指定する（サブメニューの親には ID が無いため）
    for (position, item) in items.iter().enumerate() {
        if item.is_separator() {
            append_separator(hmenu);
            continue;
        }

        if item.has_submenu() {
            let submenu = unsafe { CreatePopupMenu() };
            add_menu_items(
                submenu,
                &item.submenu,
                Arc::clone(&targets),
                state,
                icons,
                config,
            );
            append_submenu(hmenu, submenu, &item.name, item.accesskey);
        } else {
            let id = state.lock().unwrap().add_action(MenuAction::ExecuteApp {
                item: Box::new(item.clone()),
                targets: Arc::clone(&targets),
                delay: config.delay_of(item),
            });
            append_menu_item(hmenu, id, &item.name, item.accesskey);
        }

        if let Some(bitmap) = icons.bitmap_for(item) {
            set_item_bitmap(hmenu, position as u32, bitmap);
        }
    }
}

/// 項目にアイコンを付ける
///
/// `MF_OWNERDRAW` は要らない。32 ビットのビットマップを渡せば、Vista 以降の
/// メニューがアルファ込みで描いてくれる。
fn set_item_bitmap(hmenu: HMENU, position: u32, bitmap: HBITMAP) {
    let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
    info.fMask = MIIM_BITMAP;
    info.hbmpItem = bitmap;

    unsafe { SetMenuItemInfoW(hmenu, position, 1, &info) };
}

/// アイコンの読み込みと使い回し
///
/// 設定ファイルは同じ実行ファイルを何度も指す（同梱サンプルでは 1 つの拡張子に
/// 25 項目並ぶが、実行ファイルは数種類しかない）。パスと番号で覚えておけば、
/// 実際に取り出すのは数回で済む。
struct IconCache {
    mode: IconMode,
    /// アイコンの一辺（物理ピクセル）
    size: i32,
    loaded: Vec<(String, i32, Option<HBITMAP>)>,
}

impl IconCache {
    fn new(mode: IconMode, point: POINT) -> Self {
        IconCache {
            mode,
            size: if mode == IconMode::None {
                0
            } else {
                icon_size(point)
            },
            loaded: Vec::new(),
        }
    }

    /// 項目に付けるアイコン
    fn bitmap_for(&mut self, item: &MenuItem) -> Option<HBITMAP> {
        if self.mode == IconMode::None {
            return None;
        }

        match &item.icon {
            Some(spec) => self.load(&spec.path, spec.index),
            // 指定が無い項目を実行ファイルから補うのは auto のときだけ
            None if self.mode == IconMode::Auto && !item.path.is_empty() => {
                self.load(&item.path, 0)
            }
            None => None,
        }
    }

    /// 同じパスと番号なら 1 度しか読み込まない（読めなかったことも覚える）
    fn load(&mut self, path: &str, index: i32) -> Option<HBITMAP> {
        if let Some((_, _, bitmap)) = self
            .loaded
            .iter()
            .find(|(cached, cached_index, _)| cached == path && *cached_index == index)
        {
            return *bitmap;
        }

        let bitmap = crate::icon::load(Path::new(path), index, self.size);
        self.loaded.push((path.to_string(), index, bitmap));
        bitmap
    }

    /// 読み込んだビットマップを解放する（メニューを壊したあとに呼ぶ）
    fn dispose(&mut self) {
        for (_, _, bitmap) in self.loaded.drain(..) {
            if let Some(bitmap) = bitmap {
                crate::icon::dispose(bitmap);
            }
        }
    }
}

/// メニューに載せるアイコンの一辺（物理ピクセル）
///
/// Per-Monitor V2 ではメニューの文字も枠も自動で拡大されるが、**こちらが渡した
/// ビットマップは拡大されない**。出す先のモニタの DPI を見て自分で決める。
fn icon_size(point: POINT) -> i32 {
    let mut dpi = 96;

    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if !monitor.is_null() {
        let mut dpi_x = 0;
        let mut dpi_y = 0;
        if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } == 0 {
            dpi = dpi_x;
        }
    }

    unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }
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
        MenuAction::ExecuteApp {
            item,
            targets,
            delay,
        } => {
            execute_command(&item, targets.as_slice(), delay);
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

/// 進行状況ダイアログを出す待ち時間の合計（ミリ秒）
///
/// これより短い待ちのためにダイアログが一瞬出て消えるほうが煩わしいので、
/// 下回るときは黙って待つ。**判定は起動より前に確定する**ので、`--preview` に
/// 「進行状況を表示します」と書ける。
const PROGRESS_THRESHOLD_MS: u64 = 1_000;

/// 起動の進み具合
#[derive(Default)]
struct Run {
    /// 起動を試みた数（残りを数えるのに使う）
    attempted: usize,
    /// 実際に起動できた数
    started: usize,
    /// 起動できなかった理由
    reasons: Vec<String>,
    /// 進行状況を出していて、途中で止まったときの理由
    interrupted: Option<progress::Outcome>,
}

/// 待ち時間の合計から、進行状況ダイアログを出すかどうかを決める
pub fn shows_progress(delay: u32, total: usize) -> bool {
    delay > 0 && total > 1 && u64::from(delay) * (total as u64 - 1) >= PROGRESS_THRESHOLD_MS
}

/// 組み立てたプロセスを順に起動する
///
/// `delay` が 0 なら間を空けずに起動する（`:delay` を書くまでの従来どおり）。
/// 値があるときは起動と起動のあいだを空け、合計が長くなるときだけ進行状況を
/// 見せる。**待ち時間の実体は進行状況ダイアログのモーダルループ**で、そちらを
/// 使うときは `thread::sleep` を通らない（眠るとメッセージを汲めなくなる）。
fn launch_all(name: &str, invocations: &[Invocation], delay: u32) -> Run {
    let total = invocations.len();
    let mut run = Run::default();

    // 1 つぶんの起動。`false` を返すと以降を起動しない
    let mut launch = |index: usize| -> bool {
        run.attempted = index + 1;
        let invocation = &invocations[index];

        match spawn_command(
            &invocation.program,
            &invocation.args,
            &invocation.working_dir,
            invocation.admin,
        ) {
            Ok(Launch::Started) => {
                run.started += 1;
                true
            }
            // 昇格を断られたら、残りの対象も起動しない
            Ok(Launch::Cancelled) => false,
            // 起動できなかった理由は集めておいて、あとで 1 枚にまとめる
            Err(reason) => {
                run.reasons.push(reason);
                true
            }
        }
    };

    let mut interrupted = None;

    if shows_progress(delay, total) {
        match progress::run(name, total, delay, &mut launch) {
            progress::Outcome::Finished => {}
            // ダイアログを出せなかったときは、せめて間隔だけは空けて起動する
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

/// 間隔を空けながら順に起動する（進行状況は出さない）
fn launch_in_order(total: usize, delay: u32, launch: &mut dyn FnMut(usize) -> bool) {
    for index in 0..total {
        // 待つのは起動と起動の「あいだ」。1 つ目は待たせない
        if index > 0 && delay > 0 {
            std::thread::sleep(Duration::from_millis(u64::from(delay)));
        }

        if !launch(index) {
            break;
        }
    }
}

/// まだ起動していない対象のパス
///
/// `resolve_invocations` は個別実行のとき対象と同じ順・同じ個数を返すので、
/// 起動を試みた数がそのまま境目になる。`+`（まとめて渡す）は 1 プロセスなので
/// 間隔そのものが効かず、ここには来ない。
fn remaining_paths(
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

/// コマンドを実行
fn execute_command(item: &MenuItem, targets: &[Target], delay: u32) {
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

    // 入力を先に済ませる。あとの確認ダイアログで、入力した値が入った状態の
    // メッセージを見せられる（`:confirm 長辺 $?{幅} に縮小します` が書ける）
    let base = PathPlaceholders::from_path(&targets[0].path);
    if !ask_prompts(item, &base, &ctx) {
        return;
    }

    // 確認は項目に対して 1 回。対象の数だけ聞かれても答えは変わらない
    if !confirm_execution(item, targets, &ctx) {
        return;
    }

    // 昇格はプロセスごとにしかできないので、個別実行では UAC が対象の数だけ出る。
    // 何回出るかを先に知らせる（途中で断れば残りは起動しない）
    if !confirm_repeated_elevation(item, targets.len()) {
        return;
    }

    let invocations = resolve_invocations(item, targets, &ctx);
    let run = launch_all(&item.name, &invocations, delay);

    // 個別実行では対象の数だけ同じ失敗が並ぶので、集めてから 1 枚だけ出す
    show_spawn_error(&exe_path, &run.reasons);

    // 途中で止まったら、何を起動して何が残ったかを見せる。先に失敗を出すのは、
    // 起動できた数の意味がそれを知らないと読み取れないため
    if let Some(outcome) = run.interrupted {
        progress::show_summary(
            outcome,
            run.started,
            invocations.len(),
            &remaining_paths(targets, &invocations, run.attempted),
        );
    }
}

/// 起動の結果（`Cancelled` は UAC を断られた場合）
enum Launch {
    Started,
    Cancelled,
}

/// プロセスを起動する（失敗したら理由を返す）
fn spawn_command(
    exe_path: &Path,
    args: &[String],
    working_dir: &str,
    admin: bool,
) -> Result<Launch, String> {
    if admin {
        return spawn_elevated(exe_path, args, working_dir);
    }

    Command::new(exe_path)
        .args(args)
        .current_dir(working_dir)
        .spawn()
        .map(|_| Launch::Started)
        .map_err(|error| error.to_string())
}

/// 管理者として起動する（`:admin`）
///
/// `CreateProcess` には昇格の仕組みが無いので、`runas` 動詞で
/// `ShellExecuteExW` を呼ぶ。`CreateProcess` 経路と違って引数を 1 本の文字列で
/// 渡すため、`join_args` が引用符を付け直す。
fn spawn_elevated(exe_path: &Path, args: &[String], working_dir: &str) -> Result<Launch, String> {
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
    info.fMask = SEE_MASK_NOASYNC;
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
        return Ok(Launch::Started);
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
fn join_args(args: &[String]) -> String {
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
/// 実行時の失敗ダイアログと `--check` の警告が同じ判断をするよう、表はここ
/// 1 か所に置く。
pub fn needs_interpreter(exe_path: &Path) -> bool {
    let Some(extension) = exe_path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(extension.to_lowercase().as_str(), "ps1" | "vbs" | "js")
}

/// 項目の中に書かれた入力欄を、重複を除いて書かれた順に集める
///
/// 同じ書き方を 2 か所に置いても聞かれるのは 1 回。`-w $?{幅} -h $?{幅}` のような
/// 書き方が意図どおりになる。
pub fn item_prompts(item: &MenuItem) -> Vec<crate::prompt::Prompt<'_>> {
    let mut found: Vec<crate::prompt::Prompt<'_>> = Vec::new();

    let texts = item
        .args
        .iter()
        .chain(std::iter::once(&item.working_dir))
        .chain(item.confirm.iter());

    for text in texts {
        for prompt in crate::prompt::prompts(text) {
            if !found.iter().any(|found| found.source == prompt.source) {
                found.push(prompt);
            }
        }
    }

    found
}

/// `$?{...}` の答えを集める（すべて答えられたら `true`）
///
/// ひとつでもキャンセルされたら、そこで打ち切って実行しない。半端に入力した
/// ぶんだけで起動すると、意図しない引数でコマンドが走る。
fn ask_prompts(item: &MenuItem, base: &PathPlaceholders, ctx: &RunContext) -> bool {
    for prompt in item_prompts(item) {
        // 説明と既定値の中のプレースホルダーは先に解決する（`$?{$a の新しい名前}`
        // や `$?{幅=$e}` が書ける）。基準は :dir と同じく最初の対象
        let message = base.replace(prompt.message, ctx);
        let default_value = base.replace(prompt.default_value, ctx);

        match crate::prompt::ask(prompt.rule, &message, &default_value) {
            Some(value) => ctx.set_prompt(prompt.source, value),
            None => return false,
        }
    }

    true
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

/// 個別実行の `:admin` で、UAC が何回出るかを先に知らせる
///
/// 昇格はプロセスごとにしかできないので、対象の数だけ確認が出る。知らずに
/// 10 個選ぶと 10 回聞かれることになるため、その前に 1 回だけ確認する。
/// `+`（まとめて渡す）なら起動は 1 回なので聞かない。
fn confirm_repeated_elevation(item: &MenuItem, target_count: usize) -> bool {
    if !item.admin || item.all_mode || target_count < 2 {
        return true;
    }

    let body = format!(
        "「{}」を管理者として実行します。\n\n\
         対象が {} 件あるため、ユーザーアカウント制御の確認が {} 回表示されます。\n\
         （途中でキャンセルすると、残りは実行されません）\n\n実行しますか?",
        item.name, target_count, target_count
    );

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

    #[test]
    fn 管理者指定は起動の組み立てに伝わる() {
        let config = parse("[.txt]\nA | C:\\Windows\\notepad.exe\n :admin").config;
        let targets = vec![Target::from_path(PathBuf::from("C:\\x\\y.txt"))];
        let invocations = resolve_invocations(&config.apps[0], &targets, &RunContext::for_test());
        assert!(invocations[0].admin);
    }

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
            (".png", 26),
            (".jpg", 26),
            (".gif", 29),
            (".ico", 25),
            (".bmp", 26),
            (".tif", 27),
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
            // [@テキスト] には無いが「文字数・行数を数える」が [+.ps1] で足している
            // （その項目が出るぶん、[file] 冒頭の --- も先頭でなくなり残る）
            (".ps1", 18),
            // どのセクションにも該当しない拡張子は [file] と [file folder] だけ
            (".pdf", 16),
            ("file", 16),
            ("folder", 22),
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
                "長辺を指定して縮小する",
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
                "管理者としてコマンドプロンプトを開く (A)",
            ]
        );
        // 引数欄を空にした項目は引数なし、:dir はプレースホルダーを保ったまま
        assert!(open.submenu[3].args.is_empty());
        assert_eq!(open.submenu[3].working_dir, "$p");
        // :admin が付くのは最後の 1 つだけ
        assert!(!open.submenu[3].admin);
        assert!(open.submenu[4].admin);
    }

    // -----------------------------------------------------------------
    // アイコン
    //
    // 見た目は自動では確かめられないが、「ビットマップが項目に付いたか」は
    // メニューから読み戻せる。3 つのモードの違いはここで押さえられる
    // -----------------------------------------------------------------

    /// 位置で指定した項目に付いているビットマップ
    fn bitmap_at(hmenu: HMENU, position: u32) -> Option<HBITMAP> {
        let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
        info.fMask = MIIM_BITMAP;

        let read = unsafe { GetMenuItemInfoW(hmenu, position, 1, &mut info) };
        if read == 0 || info.hbmpItem.is_null() {
            None
        } else {
            Some(info.hbmpItem)
        }
    }

    /// 設定を組み立てて、各項目にビットマップが付いたかを返す
    fn icons_of(text: &str, mode: IconMode) -> Vec<bool> {
        let parsed = crate::config::parse(text);
        assert!(!parsed.has_error(), "設定にエラーがある");

        let targets = vec![target(".txt")];
        let items = filter_menu_items(&parsed.config.apps, &targets);
        let state = Arc::new(Mutex::new(GlobalState::new()));
        let hmenu = unsafe { CreatePopupMenu() };
        let mut icons = IconCache::new(mode, POINT { x: 0, y: 0 });

        add_menu_items(
            hmenu,
            &items,
            Arc::new(targets),
            &state,
            &mut icons,
            &parsed.config,
        );

        let found = (0..items.len())
            .map(|position| bitmap_at(hmenu, position as u32).is_some())
            .collect();

        unsafe { DestroyMenu(hmenu) };
        icons.dispose();
        found
    }

    /// `:icon` を書いた項目・書いていない項目・アイコンの無いファイル
    const ICON_CONFIG: &str = "[.txt]\n\
        指定あり | C:\\Windows\\notepad.exe\n :icon C:\\Windows\\System32\\imageres.dll,3\n\
        指定なし | C:\\Windows\\explorer.exe\n";

    /// 既定。`:icon` を書いた項目だけに付く
    #[test]
    fn specified_は指定した項目だけにアイコンが付く() {
        assert_eq!(
            icons_of(ICON_CONFIG, IconMode::Specified),
            vec![true, false]
        );
    }

    /// 書いてあっても出さない（設定を消さずに一時的に止められる）
    #[test]
    fn none_はどの項目にもアイコンが付かない() {
        assert_eq!(icons_of(ICON_CONFIG, IconMode::None), vec![false, false]);
    }

    /// 指定が無い項目は実行ファイルから補う
    #[test]
    fn auto_は実行ファイルからも取り出す() {
        assert_eq!(icons_of(ICON_CONFIG, IconMode::Auto), vec![true, true]);
    }

    /// サブメニューの親にも付く（:confirm と違って意味がある）
    #[test]
    fn サブメニューの親にもアイコンが付く() {
        let text = "[.txt]\n親\n :icon C:\\Windows\\System32\\imageres.dll,3\n\
                    > 子 | C:\\Windows\\notepad.exe\n";
        assert_eq!(icons_of(text, IconMode::Specified), vec![true]);
    }

    /// 同じパスと番号を何度書いても、取り出しは 1 回で済む
    #[test]
    fn 同じアイコンは使い回される() {
        let mut icons = IconCache::new(IconMode::Specified, POINT { x: 0, y: 0 });
        let path = "C:\\Windows\\System32\\imageres.dll";

        let first = icons.load(path, 3);
        let second = icons.load(path, 3);
        assert!(first.is_some());
        assert_eq!(first, second, "同じビットマップが返る");
        assert_eq!(icons.loaded.len(), 1, "覚えているのは 1 件だけ");

        // 番号が違えば別のアイコン
        icons.load(path, 4);
        assert_eq!(icons.loaded.len(), 2);

        icons.dispose();
    }

    /// 読めなかったことも覚えるので、無いファイルを何度も叩かない
    #[test]
    fn 読めないアイコンも覚える() {
        let mut icons = IconCache::new(IconMode::Specified, POINT { x: 0, y: 0 });
        assert!(icons.load("C:\\無い\\無い.dll", 0).is_none());
        assert!(icons.load("C:\\無い\\無い.dll", 0).is_none());
        assert_eq!(icons.loaded.len(), 1);
        icons.dispose();
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

    // -----------------------------------------------------------------
    // 起動の間隔（:delay）

    /// 短い待ちのためにダイアログが一瞬出て消えるのは煩わしいので、
    /// 合計がしきい値に届いたときだけ出す
    #[test]
    fn 進行状況を出すかどうかは待ち時間の合計で決まる() {
        assert!(!shows_progress(0, 20), "間隔が無ければ出さない");
        assert!(!shows_progress(500, 1), "1 つだけなら待ちが無い");
        assert!(!shows_progress(300, 3), "合計 600 ミリ秒では出さない");
        assert!(shows_progress(500, 3), "合計 1 秒で出す");
        assert!(shows_progress(300, 20), "合計 5.7 秒なら出す");
    }

    /// 待つのは起動と起動の「あいだ」なので、待ちの数は対象の数より 1 少ない
    #[test]
    fn しきい値はちょうど一秒から() {
        assert!(!shows_progress(999, 2), "999 ミリ秒では出さない");
        assert!(shows_progress(1000, 2), "1000 ミリ秒で出す");
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
}
