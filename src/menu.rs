/*!
メニューの作成と表示 (Win32 API版)
*/

use crate::Target;
use crate::config::{Config, MenuItem, MenuPosition};
use crate::confirm::{ask_prompts, confirm_execution};
use crate::filter::filter_menu_items;
use crate::icon::IconCache;
use crate::invoke::resolve_invocations;
use crate::launch::{launch_all, remaining_paths, searched_on_path, show_spawn_error};
use crate::placeholder::{PathPlaceholders, RunContext};
use crate::progress;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HBITMAP, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_DOWN,
};
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
        /// 件数が多いので確認するときの、しきい値（`Config::confirm_over_of` で解決済み）
        confirm_over: Option<u32>,
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
        show_error_dialog("情報", &crate::filter::empty_menu_message(targets));
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
                confirm_over: config.confirm_over_of(item, targets.len()),
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
pub(crate) fn to_wide_string(s: &str) -> Vec<u16> {
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
    unsafe {
        if msg == WM_TIMER && wparam == SELECT_FIRST_TIMER {
            KillTimer(hwnd, SELECT_FIRST_TIMER);
            send_key(VK_DOWN);
            return 0;
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
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

/// アクションを実行
fn execute_action(action: MenuAction) {
    match action {
        MenuAction::ExecuteApp {
            item,
            targets,
            delay,
            confirm_over,
        } => {
            execute_command(&item, targets.as_slice(), delay, confirm_over);
        }
        MenuAction::Close => {
            // 何もしない
        }
    }
}

/// コマンドを実行
fn execute_command(item: &MenuItem, targets: &[Target], delay: u32, confirm_over: Option<u32>) {
    if targets.is_empty() {
        return;
    }

    // 区切りを含まない名前は PATH から探されるので、ここで存在を確かめない
    // （確かめるとカレントフォルダ基準になり、PATH にあるものまで撥ねてしまう）
    let exe_path = PathBuf::from(&item.path);
    if !searched_on_path(&exe_path) && !exe_path.exists() {
        show_error_dialog(
            "エラー",
            &format!("実行ファイルが見つかりません:\n{}", exe_path.display()),
        );
        return;
    }

    // 日時はここで 1 回だけ確定させる。対象ごとに取り直すと、個別に起動した
    // ときに $t{ss} がずれて、まとめて作ったはずのファイル名が揃わなくなる
    let ctx = RunContext::capture(targets.len());

    // 入力を先に済ませる。あとの確認ダイアログで、入力した値が入った状態の
    // メッセージを見せられる（`:confirm 長辺 $?{幅} に縮小します` が書ける）
    let base = PathPlaceholders::from_path(&targets[0].path);
    if !ask_prompts(item, &base, &ctx) {
        return;
    }

    // 確認は項目に対して 1 回。対象の数だけ聞かれても答えは変わらない
    if !confirm_execution(item, targets, &ctx, confirm_over) {
        return;
    }

    let invocations = resolve_invocations(item, targets, &ctx);
    let run = launch_all(&item.name, &invocations, delay, item.wait);

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

/// エラーダイアログを表示
pub(crate) fn show_error_dialog(title: &str, message: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

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
    use crate::config::IconMode;
    use std::path::PathBuf;

    fn target(file_type: &str) -> Target {
        Target {
            file_type: file_type.to_string(),
            path: PathBuf::from(format!("C:\\x\\y{}", file_type)),
        }
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
}
