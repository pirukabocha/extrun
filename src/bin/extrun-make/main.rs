/*!
設定づくり ― ExtRun

設定ファイルの数行を組み立てて渡すだけの道具。**編集はしない。**
フォームに入力すると設定が組み上がり、テキスト欄に出て、クリップボードに入る。
既存の設定ファイルには書き戻さない（コメントや整形を壊さないため）。

**ExtRun 本体とは独立している。** これが無くても設定は手で書ける、という
関係を保つ。ツールで作れない項目は手で書けばよい。

組み立ての中身は `form.rs` にあり、ここは `Form` を読み書きする入れ物。
画面の寸法は `layout.rs`。
*/

mod clip;
mod existing;
mod form;
mod iconpick;
mod layout;
mod live;
mod presets;

use std::ptr::null_mut;

use extrun::dialog::{show_modal, to_wide};
use form::{ExtStyle, Form, Placement, Spot, WhenKind};
use layout::*;
use live::Count;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
// SetScrollInfo だけが Controls 側にある（SCROLLINFO と GetScrollInfo は
// WindowsAndMessaging）。`Win32_UI_Controls_Dialogs` が親を引き込むので
// フィーチャーは増えない
use windows_sys::Win32::UI::Controls::SetScrollInfo;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, GetSystemMetricsForDpi,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// 入力欄の中身を差し替えるメッセージ（`windows-sys` が出さない）
const EM_GETSEL: u32 = 0x00B0;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SETSEL: u32 = 0x00B1;

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;

/// `layout.rs` と揃えておく寸法（ダイアログ単位）
const MARGIN_DLU: i16 = 8;
const BUTTON_DLU: i16 = 14;
/// スクロールバーの矢印 1 回ぶん
const LINE_DLU: i16 = 12;

/// ホイールの 1 ノッチ
const WHEEL_DELTA: i32 = 120;

/// 「対象の種類」コンボの 1 行が何を指すか
///
/// **見出しの行を混ぜるので、位置と中身が 1 対 1 でなくなる。**
/// `CBS_DROPDOWNLIST` には見出しの仕組みが無いので、選べない行として
/// 並べておいて、選ばれたら選び直す。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExtKind {
    /// 自分で指定（拡張子の欄へフォーカスを移す）
    Custom,
    /// 灰色の見出し行（選ばせない）
    Header,
    /// 設定ファイルの別名
    Alias(usize),
    /// 組み込みのひな型
    Preset(usize),
}

/// 別名を挿し込める欄
///
/// **プレースホルダーを書く欄と、パスを書く欄。** 名前欄に別名を書く意味は
/// ほとんど無いので入れていない。
const ALIAS_TARGETS: &[u16] = &[ID_APP, ID_ARGS, ID_EXT, ID_DIR, ID_ICON];

/// 画面が持っている状態
struct App {
    form: Form,
    /// 詳細設定を開いているか
    expanded: bool,
    /// 畳んだとき / 開いたときのダイアログの高さ（ダイアログ単位）
    folded_height: i16,
    expanded_height: i16,
    /// 組み立て直しを止める（値を流し込んでいる最中に鳴る通知を無視する）
    updating: bool,
    /// 「この設定で起動されるもの」で試す対象のパスと、何個選んだことにするか
    try_path: String,
    count: Count,

    // --- スクロール ---
    /// 中身の高さ（ピクセル）。詳細設定の開閉で変わる
    virtual_px: i32,
    /// いま何ピクセルぶん送っているか
    scroll: i32,
    /// 今ある設定ファイルから読んだもの（読むだけ・書き戻さない）
    existing: existing::Existing,
    /// 「対象の種類」コンボの行が何を指すか
    ext_kinds: Vec<ExtKind>,
    /// 別名を挿し込む先（最後にフォーカスがあった欄）
    last_edit: u16,

    /// スクロールバーが出ていないときのウィンドウの幅
    ///
    /// **クライアント領域の幅で覚えてはいけない。** スクロールバーが出たり
    /// 消えたりするたびにクライアント幅が変わるので、そこから逆算すると
    /// 開閉のたびにウィンドウが痩せていく（実際に 1418 → 1397 になった）。
    base_window_width: i32,
}

impl App {
    /// いまの中身の高さ（ダイアログ単位）
    fn virtual_dlu(&self) -> i16 {
        if self.expanded {
            self.expanded_height
        } else {
            self.folded_height
        }
    }
}

fn main() {
    // ウィンドウを作る前に宣言する（`extrun` 本体と同じ理由）
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    // **画面に収まるかは考えずに組み立てる。** 入りきらないぶんはスクロールで
    // 見せるので、欄を縮める必要が無い
    let template = layout::build();

    let mut app = App {
        form: Form::default(),
        expanded: false,
        folded_height: template.folded_height,
        expanded_height: template.expanded_height,
        updating: false,
        try_path: live::DEFAULT_TARGET.to_string(),
        count: Count::One,
        existing: existing::Existing::load(),
        ext_kinds: Vec::new(),
        last_edit: ID_APP,
        virtual_px: 0,
        scroll: 0,
        base_window_width: 0,
    };

    let result = show_modal(
        &template.words,
        Some(dialog_proc),
        &mut app as *mut App as LPARAM,
    );

    // 組み立てを誤ると -1 が返るだけで理由が出ないので、黙って捨てない
    if result == -1 {
        extrun::show_error_dialog(
            "設定づくり",
            "画面を組み立てられませんでした。\nExtRun の作者に知らせてください。",
        );
    }
}

// ---------------------------------------------------------------------------
// ダイアログ手続き
// ---------------------------------------------------------------------------

unsafe fn app_of(hwnd: HWND) -> *mut App {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App }
}

unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    unsafe {
        match msg {
            WM_INITDIALOG => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam);
                let app = &mut *(lparam as *mut App);
                init(hwnd, app);
                1
            }

            WM_COMMAND => {
                let app = app_of(hwnd);
                if app.is_null() {
                    return 0;
                }
                let app = &mut *app;

                let id = (wparam & 0xFFFF) as u16;
                let notify = ((wparam >> 16) & 0xFFFF) as u32;

                // Tab で送ったフォーカスが画面の外に出ないよう追いかける
                if matches!(notify, EN_SETFOCUS | BN_SETFOCUS | CBN_SETFOCUS) {
                    ensure_visible(hwnd, app, lparam as HWND);
                }
                // 別名を挿し込む先を覚えておく
                if notify == EN_SETFOCUS && ALIAS_TARGETS.contains(&id) {
                    app.last_edit = id;
                }

                command(hwnd, app, id, notify)
            }

            WM_VSCROLL => {
                let app = app_of(hwnd);
                if app.is_null() {
                    return 0;
                }
                on_vscroll(hwnd, &mut *app, (wparam & 0xFFFF) as u32);
                0
            }

            WM_MOUSEWHEEL => {
                let app = app_of(hwnd);
                if app.is_null() {
                    return 0;
                }
                let app = &mut *app;
                // 上へ回すと正の値。3 行ぶん送るのが Windows の作法
                let notches = ((wparam >> 16) as i16) as i32;
                let step = dlu_y(hwnd, LINE_DLU) * 3;
                scroll_to(hwnd, app, app.scroll - notches * step / WHEEL_DELTA);
                0
            }

            _ => 0,
        }
    }
}

/// 画面を初期状態にする
unsafe fn init(hwnd: HWND, app: &mut App) {
    unsafe {
        app.updating = true;

        // --- コンボの中身 ---
        let (kinds, labels) = extension_kinds(&app.existing);
        app.ext_kinds = kinds;
        combo_fill(hwnd, ID_EXT_KIND, &labels);

        combo_fill(hwnd, ID_ALIAS, &alias_choices(&app.existing));
        combo_select(hwnd, ID_ALIAS, 0);
        set_text(hwnd, ID_EXISTING, &app.existing.status());
        if app.existing.aliases.is_empty() {
            enable(hwnd, ID_ALIAS, false);
            enable(hwnd, ID_ALIAS_INSERT, false);
        }
        combo_fill(hwnd, ID_PLACE, &place_choices(&app.existing));
        combo_fill(
            hwnd,
            ID_WHEN,
            &[
                "いつも表示する".to_string(),
                "1 つだけ選んだとき（single）".to_string(),
                "2 つ以上選んだとき（multi）".to_string(),
            ],
        );
        combo_fill(hwnd, ID_INSERT_LIST, &insert_choices());
        combo_select(hwnd, ID_INSERT_LIST, 0);

        write_form(hwnd, &app.form);
        set_text(hwnd, ID_TRY_PATH, &app.try_path);
        check(hwnd, ID_COUNT_ONE, true);
        app.updating = false;

        // 作られたときのウィンドウの幅を控えておく。スクロールバーが要る
        // ときはこれに幅を足す（内側に食い込ませると、いちばん右のグループ枠の
        // 縁が削れる）
        let mut window = zero_rect();
        GetWindowRect(hwnd, &mut window);
        app.base_window_width = window.right - window.left;

        // 詳細設定は畳んだ状態から始める
        fold(hwnd, app, false);
        rebuild(hwnd, app);

        // 最初に触るのは名前の欄
        SetFocus(GetDlgItem(hwnd, ID_NAME as i32));
    }
}

/// ボタンや欄からの通知をさばく
unsafe fn command(hwnd: HWND, app: &mut App, id: u16, notify: u32) -> isize {
    unsafe {
        match id {
            _ if id == IDCANCEL as u16 => {
                EndDialog(hwnd, 0);
                return 1;
            }

            ID_FOLD => {
                let expanded = !app.expanded;
                fold(hwnd, app, expanded);
                return 1;
            }

            ID_COPY => {
                let text = app.form.to_config();
                if clip::copy(hwnd, &text) {
                    // 押したことが分かるように、欄の中身を選択して見せる
                    let output = GetDlgItem(hwnd, ID_OUTPUT as i32);
                    SendMessageW(output, EM_SETSEL, 0, -1);
                    SetFocus(output);
                } else {
                    extrun::show_error_dialog(
                        "設定づくり",
                        "クリップボードを開けませんでした。\n他のアプリが使っている間は失敗することがあります。",
                    );
                }
                return 1;
            }

            ID_APP_BROWSE => {
                if let Some(path) = clip::pick_executable(hwnd) {
                    set_text(hwnd, ID_APP, &path);
                    read_form(hwnd, app);
                    rebuild(hwnd, app);
                }
                return 1;
            }

            ID_ICON_PICK => {
                // 今の値と「起動するアプリ」を渡す。後者は「よく使う」の
                // 3 つ目（その exe が持っているアイコンを見たいことが多い）
                let current = get_text(hwnd, ID_ICON);
                if let Some(picked) = iconpick::pick(hwnd, &current, &app.form.app) {
                    set_text(hwnd, ID_ICON, &picked);
                    read_form(hwnd, app);
                    rebuild(hwnd, app);
                }
                return 1;
            }

            ID_TRY_BROWSE => {
                if let Some(path) = clip::pick_any(hwnd) {
                    set_text(hwnd, ID_TRY_PATH, &path);
                    read_form(hwnd, app);
                    rebuild(hwnd, app);
                }
                return 1;
            }

            ID_INSERT => {
                insert_placeholder(hwnd, app);
                return 1;
            }

            ID_EXT_KIND if notify == CBN_SELCHANGE => {
                choose_extension_kind(hwnd, app);
                return 1;
            }

            ID_ALIAS_INSERT => {
                insert_alias(hwnd, app);
                return 1;
            }

            _ => {}
        }

        // 入力欄の書き換えとチェックの切り替えは、まとめて組み立て直しに回す
        let changed = matches!(notify, EN_CHANGE | BN_CLICKED | CBN_SELCHANGE);
        if changed && !app.updating {
            read_form(hwnd, app);
            sync_enabled(hwnd, &app.form);
            rebuild(hwnd, app);
            return 1;
        }

        0
    }
}

// ---------------------------------------------------------------------------
// フォームと画面のやりとり
// ---------------------------------------------------------------------------

/// 画面 → `Form`
unsafe fn read_form(hwnd: HWND, app: &mut App) {
    unsafe {
        let form = &mut app.form;

        form.name = get_text(hwnd, ID_NAME);
        form.key = get_text(hwnd, ID_KEY);
        form.app = get_text(hwnd, ID_APP);
        form.args = get_text(hwnd, ID_ARGS);
        form.no_args = checked(hwnd, ID_NO_ARGS);
        form.all_mode = checked(hwnd, ID_ALL_MODE);

        form.extensions = get_text(hwnd, ID_EXT);
        form.ext_style = if checked(hwnd, ID_EXT_PERITEM) {
            ExtStyle::PerItem
        } else {
            ExtStyle::Section
        };

        form.placement = placement_at(&app.existing, combo_selection(hwnd, ID_PLACE));
        form.submenu_name = get_text(hwnd, ID_SUB_NAME);
        form.submenu_key = get_text(hwnd, ID_SUB_KEY);
        form.separator = checked(hwnd, ID_SEPARATOR);

        form.confirm = checked(hwnd, ID_CONFIRM);
        form.confirm_message = get_text(hwnd, ID_CONFIRM_MESSAGE);
        form.admin = checked(hwnd, ID_ADMIN);
        form.wait = checked(hwnd, ID_WAIT);
        form.delay = checked(hwnd, ID_DELAY);
        form.delay_ms = get_text(hwnd, ID_DELAY_MS);
        form.when = match combo_selection(hwnd, ID_WHEN) {
            1 => WhenKind::Single,
            2 => WhenKind::Multi,
            _ => WhenKind::Always,
        };
        form.dir = get_text(hwnd, ID_DIR);
        form.icon = get_text(hwnd, ID_ICON);

        app.try_path = get_text(hwnd, ID_TRY_PATH);
        app.count = if checked(hwnd, ID_COUNT_THREE) {
            Count::Three
        } else {
            Count::One
        };
    }
}

/// `Form` → 画面（起動時に 1 回だけ）
unsafe fn write_form(hwnd: HWND, form: &Form) {
    unsafe {
        set_text(hwnd, ID_EXT, &form.extensions);
        set_text(hwnd, ID_DELAY_MS, &form.delay_ms);

        // ひな型に当てはまれば選んでおく。当てはまらなければ「自分で指定」
        combo_select(hwnd, ID_EXT_KIND, 0);
        combo_select(hwnd, ID_PLACE, 0);
        combo_select(hwnd, ID_WHEN, 0);
        check(hwnd, ID_EXT_SECTION, true);
    }
}

/// 効かない欄を灰色にする
///
/// 消すのではなく灰色にするのは、消すとレイアウトが動いて「さっきまであった
/// 欄が無い」という探し方をさせてしまうため。
unsafe fn sync_enabled(hwnd: HWND, form: &Form) {
    unsafe {
        enable(hwnd, ID_ARGS, !form.no_args);
        enable(hwnd, ID_INSERT_LIST, !form.no_args);
        enable(hwnd, ID_INSERT, !form.no_args);

        let new_submenu = form.placement == Placement::NewSubmenu;
        enable(hwnd, ID_SUB_NAME, new_submenu);
        enable(hwnd, ID_SUB_KEY, new_submenu);

        // 今あるサブメニューに入れるときは、必ず項目の行に書く。
        // 見出しを親と子のあいだに挟むと、そこから下の対象がまとめて変わる
        let choosable = !matches!(form.placement, Placement::Existing(_));
        enable(hwnd, ID_EXT_SECTION, choosable);
        enable(hwnd, ID_EXT_PERITEM, choosable);

        enable(hwnd, ID_CONFIRM_MESSAGE, form.confirm);
        enable(hwnd, ID_DELAY_MS, form.delay);
    }
}

/// ④ と ⑤ を組み立て直す
///
/// **④ を組み立ててから、それを ⑤ に渡す。** フォームから直接プレビューを
/// 作ると、ツールの中に「フォームの状態」と「設定の文字列」という 2 つの
/// 真実が並ぶ。貼る文字列がそのまま検証にもプレビューにも使われる形にする。
unsafe fn rebuild(hwnd: HWND, app: &App) {
    unsafe {
        let text = app.form.to_config();
        set_text(hwnd, ID_OUTPUT, &text);
        set_text(hwnd, ID_PASTE_HINT, &app.form.paste_hint());

        let used = presets::used_placeholders(&app.form.args);
        set_text(hwnd, ID_PLACEHOLDER_HINT, &used.join("　"));

        set_text(
            hwnd,
            ID_PREVIEW,
            &live::describe(
                &app.existing.prefix,
                app.form.paste_line(),
                &text,
                &app.try_path,
                app.count,
            ),
        );
    }
}

/// 詳細設定を開く / 閉じる
///
/// **コントロールは動かさない。** 最初から下端に置いてあるものを見せ隠しして、
/// 中身の高さ（`virtual_px`）を付け替えるだけ。位置が変わるのは下の帯の
/// 2 つ（説明と「閉じる」）だけで、これは開閉で置き場所そのものが変わるため。
unsafe fn fold(hwnd: HWND, app: &mut App, expanded: bool) {
    unsafe {
        // 動かす前にいちばん上へ戻す。スクロールしたままだと、下の帯を
        // 置き直すときの座標がずれる
        scroll_to(hwnd, app, 0);

        app.expanded = expanded;
        let show = if expanded { SW_SHOW } else { SW_HIDE };

        for id in DETAIL_IDS {
            ShowWindow(GetDlgItem(hwnd, *id as i32), show);
        }
        for offset in 0..DETAIL_DECOR_COUNT {
            ShowWindow(GetDlgItem(hwnd, (DETAIL_DECOR_FIRST + offset) as i32), show);
        }

        set_text(
            hwnd,
            ID_FOLD,
            if expanded {
                "詳細設定を閉じる ▲"
            } else {
                "詳細設定を開く ▼"
            },
        );

        let dlu = app.virtual_dlu();
        move_footer(hwnd, dlu_y(hwnd, dlu - MARGIN_DLU - BUTTON_DLU));
        apply_layout(hwnd, app);
    }
}

/// ダイアログ単位を縦のピクセルに直す
///
/// ウィンドウができたあとなら `MapDialogRect` が使える（作る前は使えないので、
/// テンプレートの寸法はダイアログ単位のまま組み立てている）。
unsafe fn dlu_y(hwnd: HWND, dlu: i16) -> i32 {
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 4,
            bottom: dlu as i32,
        };
        MapDialogRect(hwnd, &mut rect);
        rect.bottom
    }
}

/// ウィンドウの大きさとスクロールの範囲を、いまの中身に合わせる
///
/// **中身が画面に入りきらないときはスクロールで見せる。** かつては入りきる
/// ところまで欄を縮めていたが、フォームの行そのものは縮められないので
/// 下限があり、1920×1080 の 150% のような組み合わせでは結局はみ出していた。
/// スクロールにしたことで、その下限と「タイトルバーを画面内に留める」細工の
/// 両方が要らなくなった。
///
/// **スクロールバーが出るぶんだけウィンドウを広げる。** 内側に食い込ませると、
/// いちばん右のグループ枠の縁が削れる。
unsafe fn apply_layout(hwnd: HWND, app: &mut App) {
    unsafe {
        app.virtual_px = dlu_y(hwnd, app.virtual_dlu());

        let (mut window, mut client) = (zero_rect(), zero_rect());
        GetWindowRect(hwnd, &mut window);
        GetClientRect(hwnd, &mut client);

        let chrome_y = (window.bottom - window.top) - client.bottom;

        let work = work_area();
        let max_client = (work.bottom - work.top - chrome_y).max(1);
        let client_height = app.virtual_px.min(max_client);

        let scrolls = app.virtual_px > client_height;
        let bar = if scrolls {
            GetSystemMetricsForDpi(SM_CXVSCROLL, GetDpiForWindow(hwnd).max(96))
        } else {
            0
        };

        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            app.base_window_width + bar,
            client_height + chrome_y,
            SWP_NOMOVE | SWP_NOZORDER,
        );

        keep_inside_work_area(hwnd);
        set_scroll_range(hwnd, app, client_height);
    }
}

unsafe fn set_scroll_range(hwnd: HWND, app: &mut App, page: i32) {
    unsafe {
        app.scroll = app.scroll.clamp(0, (app.virtual_px - page).max(0));

        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: (app.virtual_px - 1).max(0),
            nPage: page.max(0) as u32,
            nPos: app.scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_VERT, &info, 1);
    }
}

/// いまの位置から `y` まで中身を送る
///
/// **`SW_SCROLLCHILDREN` で子ウィンドウごと動かす。** コントロールはすべて
/// ダイアログの子なので、この 1 回で全部まとめて動く。
unsafe fn scroll_to(hwnd: HWND, app: &mut App, y: i32) {
    unsafe {
        let mut client = zero_rect();
        GetClientRect(hwnd, &mut client);

        let y = y.clamp(0, (app.virtual_px - client.bottom).max(0));
        let delta = app.scroll - y;
        if delta == 0 {
            return;
        }
        app.scroll = y;

        ScrollWindowEx(
            hwnd,
            0,
            delta,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            SW_SCROLLCHILDREN | SW_INVALIDATE | SW_ERASE,
        );

        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_POS,
            nMin: 0,
            nMax: 0,
            nPage: 0,
            nPos: app.scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(hwnd, SB_VERT, &info, 1);
    }
}

/// スクロールバーの操作
unsafe fn on_vscroll(hwnd: HWND, app: &mut App, code: u32) {
    unsafe {
        let mut client = zero_rect();
        GetClientRect(hwnd, &mut client);
        let page = client.bottom;
        let line = dlu_y(hwnd, LINE_DLU);

        let target = match code {
            _ if code == SB_LINEUP as u32 => app.scroll - line,
            _ if code == SB_LINEDOWN as u32 => app.scroll + line,
            _ if code == SB_PAGEUP as u32 => app.scroll - page,
            _ if code == SB_PAGEDOWN as u32 => app.scroll + page,
            _ if code == SB_TOP as u32 => 0,
            _ if code == SB_BOTTOM as u32 => app.virtual_px,
            _ if code == SB_THUMBTRACK as u32 || code == SB_THUMBPOSITION as u32 => {
                // つまみは 16 ビットに収まらないことがあるので、位置は
                // SCROLLINFO から取る（wParam の上位ワードでは足りない）
                let mut info = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_TRACKPOS,
                    nMin: 0,
                    nMax: 0,
                    nPage: 0,
                    nPos: 0,
                    nTrackPos: 0,
                };
                GetScrollInfo(hwnd, SB_VERT, &mut info);
                info.nTrackPos
            }
            _ => return,
        };

        scroll_to(hwnd, app, target);
    }
}

/// フォーカスが移った先が画面の外なら、見えるところまで送る
///
/// **これが無いと Tab で送ったフォーカスが画面の外に消える。** スクロールする
/// 画面では Windows が面倒を見てくれないので、自分で追いかける。
unsafe fn ensure_visible(hwnd: HWND, app: &mut App, control: HWND) {
    unsafe {
        if control.is_null() {
            return;
        }

        let (mut rect, mut client) = (zero_rect(), zero_rect());
        GetWindowRect(control, &mut rect);
        GetClientRect(hwnd, &mut client);

        let mut corner = [rect.left, rect.top];
        ScreenToClient(hwnd, corner.as_mut_ptr() as *mut _);
        let top = corner[1];
        let bottom = top + (rect.bottom - rect.top);

        let margin = dlu_y(hwnd, MARGIN_DLU);
        if top < margin {
            scroll_to(hwnd, app, app.scroll + top - margin);
        } else if bottom > client.bottom - margin {
            scroll_to(hwnd, app, app.scroll + bottom - client.bottom + margin);
        }
    }
}

/// ウィンドウ全体を作業領域の中に収める
///
/// `DS_CENTER` は画面の中央に置くので、作業領域（タスクバーを除いた範囲）から
/// はみ出すことがある。高さは作業領域より大きくならないようにしてあるので、
/// 位置を寄せれば必ず全体が入る。
unsafe fn keep_inside_work_area(hwnd: HWND) {
    unsafe {
        let mut window = zero_rect();
        GetWindowRect(hwnd, &mut window);
        let work = work_area();

        let mut top = window.top;
        if window.bottom > work.bottom {
            top -= window.bottom - work.bottom;
        }
        if top < work.top {
            top = work.top;
        }

        if top != window.top {
            SetWindowPos(
                hwnd,
                null_mut(),
                window.left,
                top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
}

fn zero_rect() -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    }
}

/// タスクバーを除いた画面の範囲（取れなければ画面全体に倒す）
///
/// **デバッグビルドでは `EXTRUN_MAKE_WORK_HEIGHT` で高さを狭められる。**
/// スクロールが要る環境は「その画面にしては拡大率が大きい」組み合わせなので、
/// 開発機では再現しにくい。画面の解像度を変えずに確かめるための逃がしで、
/// リリースビルドには残らない。
unsafe fn work_area() -> RECT {
    unsafe {
        let mut work = zero_rect();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0) == 0 {
            work.right = GetSystemMetrics(SM_CXSCREEN);
            work.bottom = GetSystemMetrics(SM_CYSCREEN);
        }

        #[cfg(debug_assertions)]
        if let Ok(height) = std::env::var("EXTRUN_MAKE_WORK_HEIGHT") {
            if let Ok(height) = height.parse::<i32>() {
                work.bottom = work.top + height;
            }
        }

        work
    }
}

/// 下の帯（説明と「閉じる」）を高さに合わせて動かす
///
/// **開閉で位置が変わるのはこの 2 つだけ。** フォームの欄はどれも動かさない。
unsafe fn move_footer(hwnd: HWND, top: i32) {
    unsafe {
        // 説明は文字の高さぶん下げて、ボタンと中心を揃える
        move_to(hwnd, ID_FOOTER_NOTE, top + dlu_y(hwnd, 3));
        move_to(hwnd, IDCANCEL as u16, top);
    }
}

/// 横位置はそのままに、縦だけ動かす
unsafe fn move_to(hwnd: HWND, id: u16, top: i32) {
    unsafe {
        let control = GetDlgItem(hwnd, id as i32);
        let mut rect = zero_rect();
        GetWindowRect(control, &mut rect);

        let mut point = [rect.left, rect.top];
        ScreenToClient(hwnd, point.as_mut_ptr() as *mut _);

        SetWindowPos(
            control,
            null_mut(),
            point[0],
            top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

/// プレースホルダーを引数欄のカーソル位置に挿し込む
///
/// 挿したあと、**打ち替えてほしい部分を選択した状態にする**。
/// `$?{説明}` を挿して「説明」が選ばれていれば、そのまま自分の言葉を打てる。
/// 挿しっぱなしにすると、中括弧の中を探して手で選び直すことになる。
unsafe fn insert_placeholder(hwnd: HWND, app: &mut App) {
    unsafe {
        let index = combo_selection(hwnd, ID_INSERT_LIST);
        let Some((snippet, _, part)) = presets::INSERTS.get(index.max(0) as usize) else {
            return;
        };

        let args = GetDlgItem(hwnd, ID_ARGS as i32);
        SetFocus(args);

        let wide = to_wide(snippet);
        SendMessageW(args, EM_REPLACESEL, 1, wide.as_ptr() as LPARAM);

        // EM_REPLACESEL のあとはカーソルが挿した文字列の直後にいる。
        // そこから戻って範囲を決める（Win32 の位置は UTF-16 で数える）
        if !part.is_empty() {
            if let Some(offset) = snippet.find(part) {
                let end = SendMessageW(args, EM_GETSEL, 0, 0) as u32 >> 16;
                let snippet_len = snippet.encode_utf16().count() as u32;
                let before = snippet[..offset].encode_utf16().count() as u32;
                let part_len = part.encode_utf16().count() as u32;

                let start = end.saturating_sub(snippet_len) + before;
                SendMessageW(
                    args,
                    EM_SETSEL,
                    start as WPARAM,
                    (start + part_len) as LPARAM,
                );
            }
        }

        read_form(hwnd, app);
        rebuild(hwnd, app);
    }
}

// ---------------------------------------------------------------------------
// コンボの中身
// ---------------------------------------------------------------------------

/// 「対象の種類」に並べるもの
///
/// **並びは「自分で指定 → 設定ファイルの別名 → ひな型」**（書く回数の多い順）。
/// 別名の層は、該当するものが 1 つも無ければ丸ごと現れない。
///
/// **別名は `@` を外さない。** 外すと、ひな型の「画像」と設定ファイルの
/// `@画像` がどちらも「画像」と表示されて区別できなくなる。
fn extension_kinds(existing: &existing::Existing) -> (Vec<ExtKind>, Vec<String>) {
    let mut kinds = vec![ExtKind::Custom];
    let mut labels = vec![presets::CUSTOM.to_string()];

    let aliases = existing.extension_aliases();
    if !aliases.is_empty() {
        kinds.push(ExtKind::Header);
        labels.push("― 設定ファイルの別名 ―".to_string());
        for (index, alias) in aliases.iter().enumerate() {
            kinds.push(ExtKind::Alias(index));
            labels.push(format!(
                "@{}   {}",
                alias.name,
                existing::head(&alias.value, 28)
            ));
        }
    }

    kinds.push(ExtKind::Header);
    labels.push("― ひな型 ―".to_string());
    for (index, preset) in presets::PRESETS.iter().enumerate() {
        kinds.push(ExtKind::Preset(index));
        labels.push(format!("{}   {}", preset.label, preset.extensions));
    }

    (kinds, labels)
}

/// 「対象の種類」で選ばれたものを拡張子の欄へ流す
unsafe fn choose_extension_kind(hwnd: HWND, app: &mut App) {
    unsafe {
        let index = combo_selection(hwnd, ID_EXT_KIND).max(0) as usize;
        let kind = app.ext_kinds.get(index).copied().unwrap_or(ExtKind::Custom);

        match kind {
            // 見出しは選ばせない。今の拡張子に合う行へ戻す
            ExtKind::Header => {
                let back = current_ext_kind(app);
                combo_select(hwnd, ID_EXT_KIND, back);
                return;
            }
            // 拡張子の欄へ移って中身を選択状態にする
            ExtKind::Custom => {
                let field = GetDlgItem(hwnd, ID_EXT as i32);
                SetFocus(field);
                SendMessageW(field, EM_SETSEL, 0, -1);
            }
            ExtKind::Alias(alias) => {
                let name = app.existing.extension_aliases()[alias].name.clone();
                app.updating = true;
                set_text(hwnd, ID_EXT, &format!("@{}", name));
                app.updating = false;
            }
            ExtKind::Preset(preset) => {
                let extensions = presets::PRESETS[preset].extensions;
                app.updating = true;
                set_text(hwnd, ID_EXT, extensions);
                app.updating = false;
            }
        }

        read_form(hwnd, app);
        rebuild(hwnd, app);
    }
}

/// いまの拡張子に当てはまるコンボの行（無ければ「自分で指定」）
fn current_ext_kind(app: &App) -> i32 {
    let extensions = app.form.extensions.trim();

    let wanted = if let Some(name) = extensions.strip_prefix('@') {
        app.existing
            .extension_aliases()
            .iter()
            .position(|alias| alias.name == name)
            .map(ExtKind::Alias)
    } else {
        presets::find(extensions).map(ExtKind::Preset)
    };

    match wanted {
        Some(kind) => app
            .ext_kinds
            .iter()
            .position(|item| *item == kind)
            .unwrap_or(0) as i32,
        None => 0,
    }
}

/// 「置き場所」に並べるもの
///
/// **今あるサブメニューは、設定ファイルが読めたときだけ増える。**
/// 読めなかった人には最初の 2 つだけが出る。
fn place_choices(existing: &existing::Existing) -> Vec<String> {
    let mut choices = vec![
        "メニューの一番上の階層".to_string(),
        "新しいサブメニューを作る".to_string(),
    ];
    for submenu in &existing.submenus {
        choices.push(format!("「{}」の中", submenu.label));
    }
    choices
}

/// コンボの位置から置き場所を決める
fn placement_at(existing: &existing::Existing, index: i32) -> Placement {
    match index {
        1 => Placement::NewSubmenu,
        n if n >= 2 => match existing.submenus.get(n as usize - 2) {
            Some(submenu) => Placement::Existing(Spot {
                depth: submenu.depth,
                line: submenu.last_line,
                label: submenu.label.clone(),
            }),
            None => Placement::Root,
        },
        _ => Placement::Root,
    }
}

/// 「カーソル位置に別名を挿入」に並べるもの
///
/// **先頭の `@` を外して並べる。** `CBS_DROPDOWNLIST` は文字を打つと先頭一致で
/// その項目へ飛ぶが、全部が `@` で始まるとこの機能が丸ごと死ぬ。
/// 挿し込まれるのは `@名前` の方。値はパスが多いので**末尾を残して**詰める。
fn alias_choices(existing: &existing::Existing) -> Vec<String> {
    existing
        .aliases
        .iter()
        .map(|alias| format!("{}   {}", alias.name, existing::tail(&alias.value, 30)))
        .collect()
}

/// 選ばれた別名を、最後に触っていた欄のカーソル位置へ挿し込む
unsafe fn insert_alias(hwnd: HWND, app: &mut App) {
    unsafe {
        let index = combo_selection(hwnd, ID_ALIAS).max(0) as usize;
        let Some(alias) = app.existing.aliases.get(index) else {
            return;
        };

        let target = GetDlgItem(hwnd, app.last_edit as i32);
        SetFocus(target);

        let wide = to_wide(&format!("@{}", alias.name));
        SendMessageW(target, EM_REPLACESEL, 1, wide.as_ptr() as LPARAM);

        read_form(hwnd, app);
        rebuild(hwnd, app);
    }
}

/// 「挿入」の一覧に並べるもの
///
/// **そのまま挿して効く形だけ**を `presets::INSERTS` に置いてある
/// （`$t` や `$?` を単独で並べても、挿した先で何も起きない）。
fn insert_choices() -> Vec<String> {
    presets::INSERTS
        .iter()
        .map(|(snippet, meaning, _)| format!("{}   {}", snippet, meaning))
        .collect()
}

// ---------------------------------------------------------------------------
// Win32 の細かいところ
// ---------------------------------------------------------------------------

unsafe fn get_text(hwnd: HWND, id: u16) -> String {
    unsafe {
        let control = GetDlgItem(hwnd, id as i32);
        let length = GetWindowTextLengthW(control);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let written = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..written as usize])
    }
}

unsafe fn set_text(hwnd: HWND, id: u16, text: &str) {
    unsafe {
        let wide = to_wide(text);
        SetDlgItemTextW(hwnd, id as i32, wide.as_ptr());
    }
}

unsafe fn checked(hwnd: HWND, id: u16) -> bool {
    unsafe { SendMessageW(GetDlgItem(hwnd, id as i32), BM_GETCHECK, 0, 0) == 1 }
}

unsafe fn check(hwnd: HWND, id: u16, on: bool) {
    unsafe {
        SendMessageW(
            GetDlgItem(hwnd, id as i32),
            BM_SETCHECK,
            if on { 1 } else { 0 },
            0,
        );
    }
}

unsafe fn enable(hwnd: HWND, id: u16, on: bool) {
    unsafe {
        EnableWindow(GetDlgItem(hwnd, id as i32), if on { 1 } else { 0 });
    }
}

unsafe fn combo_fill(hwnd: HWND, id: u16, items: &[String]) {
    unsafe {
        let combo = GetDlgItem(hwnd, id as i32);
        SendMessageW(combo, CB_RESETCONTENT, 0, 0);
        for item in items {
            let wide = to_wide(item);
            SendMessageW(combo, CB_ADDSTRING, 0, wide.as_ptr() as LPARAM);
        }
        SendMessageW(combo, CB_SETCURSEL, 0, 0);
    }
}

unsafe fn combo_select(hwnd: HWND, id: u16, index: i32) {
    unsafe {
        SendMessageW(
            GetDlgItem(hwnd, id as i32),
            CB_SETCURSEL,
            index as WPARAM,
            0,
        );
    }
}

unsafe fn combo_selection(hwnd: HWND, id: u16) -> i32 {
    unsafe { SendMessageW(GetDlgItem(hwnd, id as i32), CB_GETCURSEL, 0, 0) as i32 }
}
