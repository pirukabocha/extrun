/*!
アイコンを選ぶ画面

`:icon パス,番号` の番号を手で数えなくて済むようにするためのもの。
imageres.dll には 369 個、shell32.dll には 335 個入っていて、番号を当てるのは
設定ファイルを書き慣れた人でも骨が折れる。

**1 項目 = アイコン 1 個ではなく「1 行ぶん」にしてある。** 素直に
`LBS_MULTICOLUMN` を使うと上から下へ並べて右へ流す横スクロールになり、
実測で imageres.dll が 123 列＝約 14 画面ぶんの横送りになった。かといって
全部を自前描画の子ウィンドウにすると、スクロール量の計算・当たり判定・
キーボード移動・フォーカス枠まで書くことになる。

1 行ぶんの高さの項目を `ceil(個数 / 列数)` 個並べ、`WM_DRAWITEM` の中で
横に並べて描けば、この分担で済む。

| 要るもの | どちらが持つか |
|---|---|
| 縦スクロールバー・ホイール | リストボックス（実測 1 ノッチ = 3 行） |
| ↑↓・PageUp/Down・Home/End | リストボックス（行単位で動く） |
| 描画が見えている行にしか来ない | リストボックス |
| ←→ キー | **自前**（列を動かす） |
| クリックした列の判定 | **自前**（x 座標から出す） |
| 1 マスだけを反転させる描画 | **自前** |

**遅延読み込みは仕掛けなしで成立する。** `WM_DRAWITEM` は見えている行にしか
来ないので、369 個のうち最初に取り出すのは 88 個ほど（実測 15 ms）。
先に全部取り出す作りにすると 157 ms 止まる。

アイコンの取り出しは `extrun::icon::extract`、つまり**メニューに出るものと
同じ関数**を使う。別の関数で取ると、選んだときに見たものと実際に出るものが
ずれる。
*/

use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicIsize, Ordering};

use extrun::config;
use extrun::dialog::{
    ATOM_BUTTON, ATOM_EDIT, ATOM_STATIC, BUTTON_HEIGHT, MARGIN, STYLE_BUTTON, STYLE_DEFAULT_BUTTON,
    STYLE_EDIT, STYLE_STATIC, push_header, push_item, to_dword_buffer, to_wide,
};
use extrun::text::expand_env;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    DrawFocusRect, FillRect, GetSysColorBrush, HDC, InvalidateRect,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, SetFocus, VK_LEFT, VK_RIGHT,
};
use windows_sys::Win32::UI::Shell::ExtractIconExW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ---------------------------------------------------------------------------
// windows-sys が Win32_UI_Controls を有効にしないと出さないもの
//
// **自前で書けばフィーチャーは増えない**（`dialog.rs` が `CBS_DROPDOWNLIST` を
// 手書きしているのと同じやり方）。
// ---------------------------------------------------------------------------

/// `WM_DRAWITEM` で届く構造体
#[repr(C)]
struct DrawItem {
    ctl_type: u32,
    ctl_id: u32,
    item_id: u32,
    item_action: u32,
    item_state: u32,
    hwnd_item: HWND,
    hdc: HDC,
    rc_item: RECT,
    item_data: usize,
}

/// `WM_MEASUREITEM` で届く構造体
#[repr(C)]
struct MeasureItem {
    ctl_type: u32,
    ctl_id: u32,
    item_id: u32,
    item_width: u32,
    item_height: u32,
    item_data: usize,
}

const ATOM_LISTBOX: u16 = 0x0083;

const LBS_NOTIFY: u32 = 0x0001;
const LBS_OWNERDRAWFIXED: u32 = 0x0010;
const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;
/// スクロールバーを**常に出す**
///
/// これが無いと、中身が 1 画面に収まるかどうかでスクロールバーが出たり消えたり
/// し、そのぶん内側の幅が変わる。列数は内側の幅から決めているので、
/// **同じファイルを読み直しただけで列数が 12 になったり 11 になったりする**
/// （試作で実際になった）。
const LBS_DISABLENOSCROLL: u32 = 0x1000;

const LB_ADDSTRING: u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_SETCURSEL: u32 = 0x0186;
const LB_GETCURSEL: u32 = 0x0188;
const LB_SETITEMHEIGHT: u32 = 0x01A0;
const LBN_SELCHANGE: u32 = 1;
const LBN_DBLCLK: u32 = 2;

const COLOR_WINDOW: i32 = 5;
const COLOR_HIGHLIGHT: i32 = 13;
const DI_NORMAL: u32 = 0x0003;

// --- コントロール ID ---

const ID_PATH: u16 = 100;
const ID_BROWSE: u16 = 101;
const ID_GRID: u16 = 102;
const ID_RESULT: u16 = 103;
const ID_PRESET_IMAGERES: u16 = 110;
const ID_PRESET_SHELL32: u16 = 111;
const ID_PRESET_APP: u16 = 112;

// --- 寸法（ダイアログ単位）---

const DIALOG_WIDTH: i16 = 380;
const GRID_HEIGHT: i16 = 240;
const LABEL_H: i16 = 9;
const EDIT_H: i16 = 14;

const STYLE_GRID: u32 = WS_CHILD
    | WS_VISIBLE
    | WS_TABSTOP
    | WS_BORDER
    | WS_VSCROLL
    | LBS_NOTIFY
    | LBS_OWNERDRAWFIXED
    | LBS_NOINTEGRALHEIGHT
    | LBS_DISABLENOSCROLL;

// ---------------------------------------------------------------------------
// アイコンの取り出しとキャッシュ
// ---------------------------------------------------------------------------

/// 1 つぶんの状態
///
/// **読めなかったことも覚える**（`icon.rs` の `IconCache` と同じ）。
/// 覚えないと、無いアイコンを描画のたびに叩きにいく。
#[derive(Clone, Copy)]
enum Slot {
    Untried,
    Failed,
    Loaded(HICON),
}

/// ファイルに入っているアイコンの数
///
/// `-1` を渡すと、取り出さずに個数だけ返る。この個数に対する `0..個数-1` が、
/// そのまま `:icon パス,番号` の番号になる。
fn count_icons(path: &str) -> u32 {
    let wide = to_wide(path);
    unsafe { ExtractIconExW(wide.as_ptr(), -1, null_mut(), null_mut(), 0) }
}

// ---------------------------------------------------------------------------
// 画面の状態
// ---------------------------------------------------------------------------

struct Picker {
    /// 打たれたままのパス（`%SystemRoot%` などを含む）
    path: String,
    /// 「よく使う」の 3 つ目に入れる、メイン画面の「起動するアプリ」
    app: String,
    slots: Vec<Slot>,
    icon_size: i32,
    cell: i32,
    cols: i32,
    rows: i32,
    selected: i32,
    /// クリックで決まった列（`LBN_SELCHANGE` がこれを使って番号を組み立てる）
    col_hint: i32,
    /// OK で確定した `パス,番号`
    picked: Option<String>,
}

impl Picker {
    /// 展開後の実際のパス
    fn resolved(&self) -> String {
        expand_env(&self.path, false)
    }

    /// 描画のときに 1 つだけ取り出す（遅延読み込みの本体）
    fn icon_at(&mut self, index: usize) -> Option<HICON> {
        match self.slots[index] {
            Slot::Loaded(icon) => Some(icon),
            Slot::Failed => None,
            Slot::Untried => {
                let resolved = PathBuf::from(self.resolved());
                match extrun::icon::extract(&resolved, index as i32, self.icon_size) {
                    Some(icon) => {
                        self.slots[index] = Slot::Loaded(icon);
                        Some(icon)
                    }
                    None => {
                        self.slots[index] = Slot::Failed;
                        None
                    }
                }
            }
        }
    }

    fn release(&mut self) {
        for slot in &mut self.slots {
            if let Slot::Loaded(icon) = *slot {
                unsafe { DestroyIcon(icon) };
            }
            *slot = Slot::Untried;
        }
    }
}

/// アイコンを選ばせて `パス,番号` を返す
///
/// `current` は今の `:icon` の値（空でもよい）、`app` はメイン画面の
/// 「起動するアプリ」。取り消したときは `None`。
pub fn pick(owner: HWND, current: &str, app: &str) -> Option<String> {
    let spec = config::parse_icon(current);
    let path = if spec.path.is_empty() {
        r"%SystemRoot%\System32\imageres.dll".to_string()
    } else {
        spec.path
    };

    let mut picker = Picker {
        path,
        app: app.trim().to_string(),
        slots: Vec::new(),
        icon_size: 32,
        cell: 48,
        cols: 1,
        rows: 0,
        selected: spec.index.max(0),
        col_hint: 0,
        picked: None,
    };

    let template = build_template();
    let result = show_modal_with_owner(
        &template,
        owner,
        Some(dialog_proc),
        &mut picker as *mut Picker as LPARAM,
    );

    if result == -1 {
        extrun::show_error_dialog("設定づくり", "アイコンの画面を組み立てられませんでした。");
        return None;
    }

    picker.picked
}

/// オーナー付きでモーダルを出す
///
/// `dialog::show_modal` はオーナーを取らない。**アイコンの画面はメイン画面の
/// 上に出す**必要があるので、ここだけ自前で呼ぶ。
fn show_modal_with_owner(template: &[u32], owner: HWND, proc: DLGPROC, data: LPARAM) -> isize {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    unsafe {
        DialogBoxIndirectParamW(
            GetModuleHandleW(null_mut()),
            template.as_ptr() as *const DLGTEMPLATE,
            owner,
            proc,
            data,
        )
    }
}

// ---------------------------------------------------------------------------
// テンプレート
// ---------------------------------------------------------------------------

fn build_template() -> Vec<u32> {
    let mut words: Vec<u16> = Vec::new();
    let content = DIALOG_WIDTH - MARGIN * 2;

    let y_path_label = MARGIN;
    let y_path = y_path_label + LABEL_H + 2;
    let y_preset_label = y_path + EDIT_H + 6;
    let y_preset = y_preset_label + LABEL_H + 2;
    let y_grid = y_preset + BUTTON_HEIGHT + 8;
    let y_result_label = y_grid + GRID_HEIGHT + 6;
    let y_result = y_result_label + LABEL_H + 2;
    let y_buttons = y_result + EDIT_H + 8;
    let height = y_buttons + BUTTON_HEIGHT + MARGIN;

    push_header(&mut words, DIALOG_WIDTH, height, "アイコンを選ぶ ― ExtRun");

    static_text(
        &mut words,
        MARGIN,
        y_path_label,
        content,
        "アイコンの入ったファイル（.ico / .exe / .dll）",
    );
    push_item(
        &mut words,
        STYLE_EDIT,
        MARGIN,
        y_path,
        content - 44,
        EDIT_H,
        ID_PATH,
        ATOM_EDIT,
        "",
    );
    push_item(
        &mut words,
        STYLE_BUTTON,
        DIALOG_WIDTH - MARGIN - 42,
        y_path,
        42,
        BUTTON_HEIGHT,
        ID_BROWSE,
        ATOM_BUTTON,
        "参照…",
    );

    static_text(&mut words, MARGIN, y_preset_label, content, "よく使う");
    push_item(
        &mut words,
        STYLE_BUTTON,
        MARGIN,
        y_preset,
        58,
        BUTTON_HEIGHT,
        ID_PRESET_IMAGERES,
        ATOM_BUTTON,
        "imageres.dll",
    );
    push_item(
        &mut words,
        STYLE_BUTTON,
        MARGIN + 62,
        y_preset,
        54,
        BUTTON_HEIGHT,
        ID_PRESET_SHELL32,
        ATOM_BUTTON,
        "shell32.dll",
    );
    push_item(
        &mut words,
        STYLE_BUTTON,
        MARGIN + 120,
        y_preset,
        70,
        BUTTON_HEIGHT,
        ID_PRESET_APP,
        ATOM_BUTTON,
        "起動するアプリ",
    );

    push_item(
        &mut words,
        STYLE_GRID,
        MARGIN,
        y_grid,
        content,
        GRID_HEIGHT,
        ID_GRID,
        ATOM_LISTBOX,
        "",
    );

    static_text(
        &mut words,
        MARGIN,
        y_result_label,
        content,
        "設定ファイルに書かれる値",
    );
    push_item(
        &mut words,
        STYLE_EDIT | ES_READONLY as u32,
        MARGIN,
        y_result,
        content,
        EDIT_H,
        ID_RESULT,
        ATOM_EDIT,
        "",
    );

    push_item(
        &mut words,
        STYLE_DEFAULT_BUTTON,
        DIALOG_WIDTH - MARGIN - 104,
        y_buttons,
        50,
        BUTTON_HEIGHT,
        IDOK as u16,
        ATOM_BUTTON,
        "OK",
    );
    push_item(
        &mut words,
        STYLE_BUTTON,
        DIALOG_WIDTH - MARGIN - 50,
        y_buttons,
        50,
        BUTTON_HEIGHT,
        IDCANCEL as u16,
        ATOM_BUTTON,
        "キャンセル",
    );

    to_dword_buffer(&words)
}

fn static_text(words: &mut Vec<u16>, x: i16, y: i16, width: i16, text: &str) {
    push_item(
        words,
        STYLE_STATIC,
        x,
        y,
        width,
        LABEL_H,
        u16::MAX,
        ATOM_STATIC,
        text,
    );
}

// ---------------------------------------------------------------------------
// 格子のリストボックスを差し替える（←→ とクリックの列判定だけ）
// ---------------------------------------------------------------------------

/// 元のリストボックスの手続き
///
/// `SetWindowSubclass` は comctl32 なので使わない。`GWLP_WNDPROC` の
/// 差し替えは user32 だけで済む。
static ORIGINAL_GRID_PROC: AtomicIsize = AtomicIsize::new(0);

unsafe fn picker_of(dialog: HWND) -> *mut Picker {
    unsafe { GetWindowLongPtrW(dialog, GWLP_USERDATA) as *mut Picker }
}

/// 番号を動かして、その行が見えるようにする
unsafe fn set_selection(dialog: HWND, picker: &mut Picker, index: i32) {
    unsafe {
        let count = picker.slots.len() as i32;
        if count == 0 {
            return;
        }
        picker.selected = index.clamp(0, count - 1);
        picker.col_hint = picker.selected % picker.cols;

        let grid = GetDlgItem(dialog, ID_GRID as i32);
        // 行を選ぶとリストボックスがそこまでスクロールしてくれる
        SendMessageW(
            grid,
            LB_SETCURSEL,
            (picker.selected / picker.cols) as WPARAM,
            0,
        );
        InvalidateRect(grid, null_mut(), 0);
        update_result(dialog, picker);
    }
}

unsafe extern "system" fn grid_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let original: WNDPROC = std::mem::transmute(ORIGINAL_GRID_PROC.load(Ordering::Relaxed));
        let dialog = GetParent(hwnd);
        let picker = picker_of(dialog);

        if !picker.is_null() {
            let picker = &mut *picker;

            match msg {
                // 左右キーはリストボックスが使わないので、ここで列に割り当てる
                WM_KEYDOWN => {
                    let vk = wparam as u32;
                    if vk == VK_LEFT as u32 {
                        set_selection(dialog, picker, picker.selected - 1);
                        return 0;
                    }
                    if vk == VK_RIGHT as u32 {
                        set_selection(dialog, picker, picker.selected + 1);
                        return 0;
                    }
                }
                // どの列を押したかは x 座標から出す。行の方はリストボックスが
                // 決めて LBN_SELCHANGE をくれるので、列だけ控えて元の手続きに渡す
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                    let x = (lparam & 0xFFFF) as i16 as i32;
                    picker.col_hint = (x / picker.cell).clamp(0, picker.cols - 1);
                }
                _ => {}
            }
        }

        CallWindowProcW(original, hwnd, msg, wparam, lparam)
    }
}

// ---------------------------------------------------------------------------
// ダイアログ手続き
// ---------------------------------------------------------------------------

unsafe fn reload(dialog: HWND, picker: &mut Picker) {
    unsafe {
        let grid = GetDlgItem(dialog, ID_GRID as i32);

        picker.release();

        // 出す先の DPI に合わせて大きさを決める。渡したビットマップは
        // 自動で拡大されないので、メニューのときと同じ事情
        let dpi = GetDpiForWindow(dialog).max(96);
        picker.icon_size = (32 * dpi / 96) as i32;
        picker.cell = picker.icon_size + (16 * dpi / 96) as i32;

        // 1 行に何個入るかは格子の内側の幅で決まる
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetClientRect(grid, &mut rect);
        picker.cols = (rect.right / picker.cell).max(1);

        let count = count_icons(&picker.resolved()) as i32;
        picker.slots = vec![Slot::Untried; count as usize];
        picker.rows = count.div_euclid(picker.cols) + i32::from(count.rem_euclid(picker.cols) != 0);
        picker.selected = picker.selected.clamp(0, (count - 1).max(0));
        picker.col_hint = picker.selected % picker.cols;

        SendMessageW(grid, LB_RESETCONTENT, 0, 0);
        SendMessageW(grid, LB_SETITEMHEIGHT, 0, picker.cell as LPARAM);

        // **1 項目 = 1 行**。369 個でも 34 行しか積まない
        for row in 0..picker.rows {
            SendMessageW(grid, LB_ADDSTRING, 0, row as LPARAM);
        }
        if picker.rows > 0 {
            SendMessageW(
                grid,
                LB_SETCURSEL,
                (picker.selected / picker.cols) as WPARAM,
                0,
            );
        }

        update_result(dialog, picker);
    }
}

unsafe fn update_result(dialog: HWND, picker: &Picker) {
    unsafe {
        let text = if picker.slots.is_empty() {
            "（このファイルにアイコンが見つかりません）".to_string()
        } else {
            format!(
                "{},{}    （{} 個中 {} 個目）",
                picker.path,
                picker.selected,
                picker.slots.len(),
                picker.selected + 1
            )
        };
        let wide = to_wide(&text);
        SetDlgItemTextW(dialog, ID_RESULT as i32, wide.as_ptr());
    }
}

/// 1 行ぶん（アイコン `cols` 個）を描く
unsafe fn draw_row(picker: &mut Picker, item: &DrawItem) {
    unsafe {
        let normal = GetSysColorBrush(COLOR_WINDOW);
        let highlight = GetSysColorBrush(COLOR_HIGHLIGHT);
        let focused = GetFocus() == item.hwnd_item;

        // リストボックスは行ごと反転させようとするが、それは無視して
        // 自分で 1 マスだけ塗る（ODS_SELECTED を見ない）
        FillRect(item.hdc, &item.rc_item, normal);

        let count = picker.slots.len() as i32;
        let cell = picker.cell;

        for col in 0..picker.cols {
            let index = item.item_id as i32 * picker.cols + col;
            if index >= count {
                break;
            }

            let rect = RECT {
                left: item.rc_item.left + col * cell,
                top: item.rc_item.top,
                right: item.rc_item.left + (col + 1) * cell,
                bottom: item.rc_item.bottom,
            };

            if index == picker.selected {
                FillRect(item.hdc, &rect, highlight);
            }

            if let Some(icon) = picker.icon_at(index as usize) {
                let x = rect.left + (cell - picker.icon_size) / 2;
                let y = rect.top + (cell - picker.icon_size) / 2;
                // メニュー用の 32bpp DIB への描き直しは要らない。
                // 自前の DC に描くだけならアルファは DrawIconEx が処理する
                DrawIconEx(
                    item.hdc,
                    x,
                    y,
                    icon,
                    picker.icon_size,
                    picker.icon_size,
                    0,
                    null_mut(),
                    DI_NORMAL,
                );
            }

            if index == picker.selected && focused {
                DrawFocusRect(item.hdc, &rect);
            }
        }
    }
}

unsafe extern "system" fn dialog_proc(
    dialog: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    unsafe {
        match msg {
            WM_INITDIALOG => {
                SetWindowLongPtrW(dialog, GWLP_USERDATA, lparam);
                let picker = &mut *(lparam as *mut Picker);

                // ←→ とクリックの列判定のためだけに差し替える
                let grid = GetDlgItem(dialog, ID_GRID as i32);
                let original =
                    SetWindowLongPtrW(grid, GWLP_WNDPROC, grid_proc as *const () as isize);
                ORIGINAL_GRID_PROC.store(original, Ordering::Relaxed);

                // こちらの入力欄でも Ctrl+A を効かせる
                crate::edits::enable_select_all(dialog);

                // 「起動するアプリ」が空なら、そのボタンは押せなくする
                if picker.app.is_empty() {
                    EnableWindow(GetDlgItem(dialog, ID_PRESET_APP as i32), 0);
                }

                let wide = to_wide(&picker.path);
                SetDlgItemTextW(dialog, ID_PATH as i32, wide.as_ptr());
                reload(dialog, picker);

                // 開いたらすぐ格子を操作できるようにする（1 を返すと
                // 最初のタブストップ＝パスの欄にフォーカスが行く）
                SetFocus(GetDlgItem(dialog, ID_GRID as i32));
                0
            }

            // オーナードローで高さが要るが、これは一覧を詰める前に 1 度だけ
            // 来るので、実際の高さは reload の LB_SETITEMHEIGHT で入れる
            WM_MEASUREITEM => {
                let measure = &mut *(lparam as *mut MeasureItem);
                measure.item_height = 48;
                1
            }

            WM_DRAWITEM => {
                let picker = picker_of(dialog);
                if picker.is_null() {
                    return 0;
                }
                let picker = &mut *picker;
                let item = &*(lparam as *const DrawItem);

                if item.ctl_id == ID_GRID as u32 && (item.item_id as i32) >= 0 {
                    draw_row(picker, item);
                }
                1
            }

            WM_COMMAND => {
                let picker = picker_of(dialog);
                if picker.is_null() {
                    return 0;
                }
                let picker = &mut *picker;

                let id = (wparam & 0xFFFF) as u16;
                let notify = ((wparam >> 16) & 0xFFFF) as u32;

                if id == ID_GRID && (notify == LBN_SELCHANGE || notify == LBN_DBLCLK) {
                    let grid = GetDlgItem(dialog, ID_GRID as i32);
                    let row = SendMessageW(grid, LB_GETCURSEL, 0, 0) as i32;
                    // 行はリストボックスが決め、列は控えておいたものを使う
                    set_selection(dialog, picker, row * picker.cols + picker.col_hint);
                    if notify == LBN_DBLCLK {
                        finish(dialog, picker);
                    }
                    return 1;
                }

                match id {
                    ID_BROWSE => {
                        if let Some(path) = crate::clip::pick_icon_source(dialog) {
                            picker.path = path;
                            let wide = to_wide(&picker.path);
                            SetDlgItemTextW(dialog, ID_PATH as i32, wide.as_ptr());
                            picker.selected = 0;
                            reload(dialog, picker);
                        }
                        1
                    }
                    ID_PRESET_IMAGERES | ID_PRESET_SHELL32 | ID_PRESET_APP => {
                        picker.path = match id {
                            ID_PRESET_IMAGERES => r"%SystemRoot%\System32\imageres.dll".to_string(),
                            ID_PRESET_SHELL32 => r"%SystemRoot%\System32\shell32.dll".to_string(),
                            _ => picker.app.clone(),
                        };
                        let wide = to_wide(&picker.path);
                        SetDlgItemTextW(dialog, ID_PATH as i32, wide.as_ptr());
                        picker.selected = 0;
                        reload(dialog, picker);
                        1
                    }
                    ID_PATH if notify == EN_CHANGE => {
                        // 打ち替えたその場では読み込み直さない。1 文字ごとに
                        // 369 個のファイルを開きにいくことになる
                        0
                    }
                    _ => {
                        if id == IDOK as u16 {
                            // 打ち替えたパスをここで拾う
                            let mut buffer = [0u16; 1024];
                            let len = GetDlgItemTextW(
                                dialog,
                                ID_PATH as i32,
                                buffer.as_mut_ptr(),
                                buffer.len() as i32,
                            ) as usize;
                            let typed = String::from_utf16_lossy(&buffer[..len]);
                            if typed.trim() != picker.path.trim() {
                                picker.path = typed;
                                picker.selected = 0;
                                reload(dialog, picker);
                                // 読み込み直した直後は選び直してもらう
                                return 1;
                            }
                            finish(dialog, picker);
                            return 1;
                        }
                        if id == IDCANCEL as u16 {
                            EndDialog(dialog, 0);
                            return 1;
                        }
                        0
                    }
                }
            }

            WM_DESTROY => {
                let picker = picker_of(dialog);
                if !picker.is_null() {
                    (*picker).release();
                }
                0
            }

            _ => 0,
        }
    }
}

/// 選んだ値を持ち帰って閉じる
unsafe fn finish(dialog: HWND, picker: &mut Picker) {
    unsafe {
        if !picker.slots.is_empty() {
            picker.picked = Some(format!("{},{}", picker.path.trim(), picker.selected));
        }
        EndDialog(dialog, 1);
    }
}
