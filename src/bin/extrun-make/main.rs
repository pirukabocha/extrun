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
mod form;
mod iconpick;
mod layout;
mod live;
mod metrics;
mod presets;

use std::ptr::null_mut;

use extrun::dialog::{show_modal, to_wide};
use form::{ExtStyle, Form, Placement, WhenKind};
use layout::Elastic;
use layout::*;
use live::Count;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// 入力欄の中身を差し替えるメッセージ（`windows-sys` が出さない）
const EM_GETSEL: u32 = 0x00B0;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SETSEL: u32 = 0x00B1;

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;

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
    /// ⑤ で試す対象のパスと、何個選んだことにするか
    try_path: String,
    count: Count,
}

fn main() {
    // ウィンドウを作る前に宣言する（`extrun` 本体と同じ理由）
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    // **画面に収まる範囲でいちばん大きく取る。** ウィンドウを作る前に測るので、
    // テンプレートは 1 回で正しい形になり、あとから縮めて周りを動かさずに済む
    let available = metrics::available_height_dlu(
        (DS_MODALFRAME | DS_SETFONT | DS_CENTER) as u32
            | WS_POPUP
            | WS_CAPTION
            | WS_SYSMENU
            | WS_MINIMIZEBOX,
        WS_EX_APPWINDOW,
    );
    let template = layout::build(Elastic::fit(available));

    let mut app = App {
        form: Form::default(),
        expanded: false,
        folded_height: template.folded_height,
        expanded_height: template.expanded_height,
        updating: false,
        try_path: live::DEFAULT_TARGET.to_string(),
        count: Count::One,
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
                command(hwnd, app, id, notify)
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
        combo_fill(hwnd, ID_EXT_KIND, &extension_kinds());
        combo_fill(
            hwnd,
            ID_PLACE,
            &[
                "メニューの一番上の階層".to_string(),
                "新しいサブメニューを作る".to_string(),
            ],
        );
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
                let index = combo_selection(hwnd, ID_EXT_KIND);
                // 0 は「自分で指定」。拡張子の欄へ移って中身を選択状態にする
                if index == 0 {
                    let field = GetDlgItem(hwnd, ID_EXT as i32);
                    SetFocus(field);
                    SendMessageW(field, EM_SETSEL, 0, -1);
                } else if let Some(preset) = presets::PRESETS.get(index as usize - 1) {
                    app.updating = true;
                    set_text(hwnd, ID_EXT, preset.extensions);
                    app.updating = false;
                }
                read_form(hwnd, app);
                rebuild(hwnd, app);
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

        form.placement = match combo_selection(hwnd, ID_PLACE) {
            1 => Placement::NewSubmenu,
            _ => Placement::Root,
        };
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
        let kind = presets::find(&form.extensions).map_or(0, |index| index as i32 + 1);
        combo_select(hwnd, ID_EXT_KIND, kind);
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

        let submenu = form.placement == Placement::NewSubmenu;
        enable(hwnd, ID_SUB_NAME, submenu);
        enable(hwnd, ID_SUB_KEY, submenu);

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
        set_text(hwnd, ID_PASTE_HINT, app.form.paste_hint());

        let used = presets::used_placeholders(&app.form.args);
        set_text(hwnd, ID_PLACEHOLDER_HINT, &used.join("　"));

        set_text(
            hwnd,
            ID_PREVIEW,
            &live::describe(&text, &app.try_path, app.count),
        );
    }
}

/// 詳細設定を開く / 閉じる
///
/// **コントロールは動かさない。** 最初から下端に置いてあるものを見せ隠しして、
/// ダイアログの高さだけを付け替える。位置を計算し直すと、開閉のたびに
/// レイアウトがずれる余地ができる。
unsafe fn fold(hwnd: HWND, app: &mut App, expanded: bool) {
    unsafe {
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

        // 下の帯は開閉に合わせて動かす（この 2 つだけは位置が変わる）
        let dlu = if expanded {
            app.expanded_height
        } else {
            app.folded_height
        };
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 4,
            bottom: dlu as i32,
        };
        MapDialogRect(hwnd, &mut rect);
        resize(hwnd, rect.bottom);

        let footer_dlu = dlu - 8 - 14;
        let mut footer = RECT {
            left: 0,
            top: 0,
            right: 4,
            bottom: footer_dlu as i32,
        };
        MapDialogRect(hwnd, &mut footer);
        move_footer(hwnd, footer.bottom);
    }
}

/// クライアント領域の高さを付け替える
///
/// **画面からはみ出すなら、はみ出したぶんだけ ④ の欄を低くする。**
/// 1920×1080 の 150% のように「その画面にしては拡大率が大きい」組み合わせでは、
/// 開いた状態が作業領域を 60〜80 px 超える。伸縮してよいのは ④ だけで、
/// フォームの行は縮めない（読めなくなる）。
unsafe fn resize(hwnd: HWND, client_height: i32) {
    unsafe {
        let mut window = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(hwnd, &mut window);
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetClientRect(hwnd, &mut client);

        let chrome = (window.bottom - window.top) - client.bottom;
        let mut wanted = client_height + chrome;

        let mut work = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0) != 0 {
            let available = work.bottom - work.top;
            if wanted > available {
                shrink_output(hwnd, wanted - available);
                wanted = available;
            }
        }

        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            window.right - window.left,
            wanted,
            SWP_NOMOVE | SWP_NOZORDER,
        );

        keep_title_bar_reachable(hwnd);
    }
}

/// タイトルバーが画面の上に隠れないようにする
///
/// **画面に収まりきらない環境がある。** 3 列の高さと詳細設定の帯は縮められない
/// ので、伸縮する 2 つの欄を下限まで削っても、1920×1080 の 150% のような
/// 組み合わせでは開いた状態がはみ出す。
///
/// `DS_CENTER` は画面の中央に置くので、そのままだとタイトルバーが上端より
/// 外に出て**マウスで動かせなくなる**。下にはみ出すぶんには、Esc で閉じられるし
/// 詳細設定を畳めば戻る。上に出さないことだけを守る。
unsafe fn keep_title_bar_reachable(hwnd: HWND) {
    unsafe {
        let mut window = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(hwnd, &mut window);

        let mut work = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0) == 0 {
            return;
        }

        if window.top < work.top {
            SetWindowPos(
                hwnd,
                null_mut(),
                window.left,
                work.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
}

/// ④ の欄を低くする（下限は 3 行ぶん）
unsafe fn shrink_output(hwnd: HWND, by: i32) {
    unsafe {
        let output = GetDlgItem(hwnd, ID_OUTPUT as i32);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(output, &mut rect);

        let mut line = RECT {
            left: 0,
            top: 0,
            right: 4,
            bottom: 24,
        };
        MapDialogRect(hwnd, &mut line);

        let height = (rect.bottom - rect.top - by).max(line.bottom);
        SetWindowPos(
            output,
            null_mut(),
            0,
            0,
            rect.right - rect.left,
            height,
            SWP_NOMOVE | SWP_NOZORDER,
        );
    }
}

/// 下の帯（説明と「閉じる」）を高さに合わせて動かす
///
/// **開閉で位置が変わるのはこの 2 つだけ。** フォームの欄はどれも動かさない。
unsafe fn move_footer(hwnd: HWND, top: i32) {
    unsafe {
        // 説明は文字の高さぶん下げて、ボタンと中心を揃える
        move_to(hwnd, ID_FOOTER_NOTE, top + 3);
        move_to(hwnd, IDCANCEL as u16, top);
    }
}

/// 横位置はそのままに、縦だけ動かす
unsafe fn move_to(hwnd: HWND, id: u16, top: i32) {
    unsafe {
        let control = GetDlgItem(hwnd, id as i32);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
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
/// **並びは「自分で指定 → ひな型」**（書く回数の多い順）。
/// 設定ファイルから読んだ別名は Phase 5 でこのあいだに入る。
fn extension_kinds() -> Vec<String> {
    let mut kinds = vec![presets::CUSTOM.to_string()];
    for preset in presets::PRESETS {
        kinds.push(format!("{}   {}", preset.label, preset.extensions));
    }
    kinds
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
