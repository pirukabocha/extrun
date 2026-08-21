/*!
画面の組み立て（`DLGTEMPLATE`）

**3 列にしてあるのは縦を押さえるため。** 2 段組みで縦に積むと、
1920×1080 の 125%（15 型ノートの既定によくある組み合わせ）で詳細設定を
開いた瞬間に画面からはみ出す。ダイアログ単位を実測して確かめてある。

1 列あたりの幅は 2 段組みのときとほぼ同じ（490 dlu を 2 分割 ≒ 700 dlu を
3 分割）なので、欄が狭くなってはいない。

    ┌ 作成した設定 ─────────────────────────────────────┐
    │ 末尾に貼り付ければ…            [クリップボードにコピー] │
    │ [.png .jpg]                                            │
    │ + ZIP にまとめる (&Z) | %SystemRoot%\...\tar.exe        │
    └────────────────────────────────────────────────────────┘
    ┌① どんな項目に──┐ ┌② どのファイルで─┐ ┌ 起動されるもの ─┐
    │                  │ └──────────────────┘ │                  │
    │                  │ ┌③ メニューのどこに┐ │                  │
    └──────────────────┘ └──────────────────┘ └──────────────────┘
    ---------------- 詳細設定を開く ▼ ----------------
    ┌実行のしかた──────┐ ┌場所と見た目──────┐ ┌表示の条件────────┐
    └──────────────────┘ └──────────────────┘ └──────────────────┘
                                                     閉じる

**すべてグループ枠で囲む。** 3 列に分かれていると、どこからどこまでが
1 つのまとまりなのかが見えないため。枠の見出しが段の見出しを兼ねるので、
独立した見出しのラベルは置かない。

**番号は入力する 3 つだけに付ける。** 結果の 2 枚（作成した設定・起動される
もの）は題名で足りるし、番号を振ると「① 作成した設定」のように最初にやる
ことではないものが先頭に来てしまう。

**「作成した設定」は上部に横断させる。** 理由は 2 つある。

- **中身が横に長い。** `+ ZIP にまとめる (&Z) | %SystemRoot%\System32\tar.exe
  | -a -c -f $d\images.zip $p` のような行は、列の幅（210 dlu）では折り返して
  読めない。逆に「起動されるもの」は 1 行が短くて行数が多いので、縦に長い
  列の方が合う
- **開閉で位置が動かない。** 下部に置くと、詳細設定を開いたときに
  **いちばん目で追う欄が跳ねる**（下に置いてなお動かさないためには、
  詳細設定より上に置くことになり、今度は「入力 → 結果 → また入力」という
  並びになる）

**列の高さは揃える。** 「起動されるもの」の高さは他の列に合わせて決めるので
（`preview_ideal`）、下に死んだ空白ができない。

詳細設定は**下に伸ばす**。横に 4 列目として開くと幅が 920 dlu になり、
100% の 1366 幅と 150% の 1920 幅で今度は横が溢れる。
*/

use extrun::dialog::{
    ATOM_BUTTON, ATOM_COMBOBOX, ATOM_EDIT, ATOM_STATIC, BUTTON_HEIGHT, STYLE_BUTTON,
    STYLE_COMBOBOX, STYLE_DEFAULT_BUTTON, STYLE_EDIT, STYLE_STATIC, push_header_with, push_item,
    to_dword_buffer,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// --- コントロール ID ---

pub const ID_NAME: u16 = 100;
pub const ID_KEY: u16 = 101;
pub const ID_APP: u16 = 102;
pub const ID_APP_BROWSE: u16 = 103;
pub const ID_ARGS: u16 = 104;
pub const ID_INSERT_LIST: u16 = 105;
pub const ID_INSERT: u16 = 106;
pub const ID_PLACEHOLDER_HINT: u16 = 107;
pub const ID_NO_ARGS: u16 = 108;
pub const ID_ALL_MODE: u16 = 109;

pub const ID_EXT_KIND: u16 = 110;
pub const ID_EXT: u16 = 111;
pub const ID_EXT_SECTION: u16 = 112;
pub const ID_EXT_PERITEM: u16 = 113;

pub const ID_PLACE: u16 = 120;
pub const ID_SUB_NAME: u16 = 121;
pub const ID_SUB_KEY: u16 = 122;
pub const ID_SEPARATOR: u16 = 123;

pub const ID_PASTE_HINT: u16 = 130;
pub const ID_OUTPUT: u16 = 131;
pub const ID_COPY: u16 = 132;

pub const ID_TRY_PATH: u16 = 133;
pub const ID_TRY_BROWSE: u16 = 134;
pub const ID_COUNT_ONE: u16 = 135;
pub const ID_COUNT_THREE: u16 = 136;
pub const ID_PREVIEW: u16 = 137;

pub const ID_FOLD: u16 = 140;
/// 下の帯の説明（開閉に合わせて動かすので ID が要る）
pub const ID_FOOTER_NOTE: u16 = 141;

pub const ID_CONFIRM: u16 = 150;
pub const ID_CONFIRM_MESSAGE: u16 = 151;
pub const ID_ADMIN: u16 = 152;
pub const ID_WAIT: u16 = 153;
pub const ID_DELAY: u16 = 154;
pub const ID_DELAY_MS: u16 = 155;
pub const ID_DIR: u16 = 156;
pub const ID_ICON: u16 = 157;
pub const ID_ICON_PICK: u16 = 159;
pub const ID_WHEN: u16 = 158;

/// 詳細設定を開いたときだけ見せるもの
///
/// **1 か所に並べておく。** 隠す・見せるの切り替えと、開いたときの高さの
/// 計算がずれると「開いたのに何も出ない」になる。
pub const DETAIL_IDS: &[u16] = &[
    ID_CONFIRM,
    ID_CONFIRM_MESSAGE,
    ID_ADMIN,
    ID_WAIT,
    ID_DELAY,
    ID_DELAY_MS,
    ID_DIR,
    ID_ICON,
    ID_ICON_PICK,
    ID_WHEN,
];

/// 詳細設定の中のラベルや枠（ID を持たないものは隠せないので ID を振る）
pub const DETAIL_DECOR_FIRST: u16 = 200;
pub const DETAIL_DECOR_COUNT: u16 = 12;

// --- 寸法（ダイアログ単位）---

pub const DIALOG_WIDTH: i16 = 700;
const MARGIN: i16 = 8;
const COL_W: i16 = 222;
const COL_GAP: i16 = 9;
const LABEL_H: i16 = 9;
const EDIT_H: i16 = 14;
const CHECK_H: i16 = 10;
/// グループ枠の見出しのぶんの空き（枠の上端から中身の先頭まで）
const GROUP_TOP: i16 = 12;
/// グループ枠の内側の左右の余白
const GROUP_PAD: i16 = 6;
/// グループ枠の下端の空き
const GROUP_BOTTOM: i16 = 8;
/// グループの中身の幅
const INNER_W: i16 = COL_W - GROUP_PAD * 2;
/// ラベルと欄のあいだ
const TIGHT: i16 = 2;
/// 欄と次のラベルのあいだ
const LOOSE: i16 = 6;

const fn col_x(index: i16) -> i16 {
    MARGIN + index * (COL_W + COL_GAP)
}

/// グループ枠のスタイル（`windows-sys` が出さないので手書き）
const BS_GROUPBOX: u32 = 0x0000_0007;
const STYLE_GROUP: u32 = WS_CHILD | WS_VISIBLE | BS_GROUPBOX;
const STYLE_CHECK: u32 = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32;
/// グループの最初のラジオ（`WS_GROUP` で仲間の区切りを作る）
const STYLE_RADIO_FIRST: u32 =
    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_GROUP | BS_AUTORADIOBUTTON as u32;
const STYLE_RADIO: u32 = WS_CHILD | WS_VISIBLE | BS_AUTORADIOBUTTON as u32;
/// 読み取り専用の複数行欄（作成した設定）
///
/// `ES_READONLY` でも選択して Ctrl+C はできる。書き換えられるようにすると、
/// 打ち込んだ内容が次の再組み立てで消えるので読み取り専用にしてある。
const STYLE_OUTPUT: u32 = WS_CHILD
    | WS_VISIBLE
    | WS_BORDER
    | WS_TABSTOP
    | WS_VSCROLL
    | ES_MULTILINE as u32
    | ES_READONLY as u32;
/// 折り返す複数行の入力欄（引数）
///
/// `ES_AUTOHSCROLL` を付けないと折り返す。`ES_WANTRETURN` を付けないので
/// Enter は既定のボタンに行き、**改行は入らない**（引数は 1 行のもの）。
const STYLE_WRAP: u32 = WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_MULTILINE as u32;

/// 伸縮してよい 2 つの欄の高さ
///
/// **伸縮してよいのはこの 2 つだけ。** フォームの行を縮めると読めなくなる。
/// 画面に収まる範囲でできるだけ大きく取り、収まらない環境では下限まで縮める。
#[derive(Debug, Clone, Copy)]
pub struct Elastic {
    /// ④ 作成した設定
    pub output: i16,
    /// ⑤ この設定で起動されるもの
    pub preview: i16,
}

impl Elastic {
    /// 「作成した設定」のゆとりのある高さ（6 行ぶんほど）
    const OUTPUT_IDEAL: i16 = 60;
    /// これ以上は縮めない（3 行ぶん）
    const FLOOR: i16 = 24;

    /// ゆとりのある画面での大きさ
    ///
    /// **プレビューの高さは他の列に合わせて決まる。** 決め打ちにすると、
    /// 列の下に死んだ空白ができるか、逆に 1 列だけ飛び出す。
    pub fn ideal() -> Elastic {
        Elastic {
            output: Elastic::OUTPUT_IDEAL,
            preview: preview_ideal(),
        }
    }
}

/// 組み立てたテンプレートと、切り替えに要る高さ
pub struct Template {
    pub words: Vec<u32>,
    /// 詳細設定を畳んだときのダイアログの高さ（ダイアログ単位）
    pub folded_height: i16,
    /// 開いたときの高さ
    pub expanded_height: i16,
}

/// 3 列がいちばん高いところ
fn columns_height(elastic: Elastic) -> i16 {
    [column1_height(), column2_height(), column3_height(elastic)]
        .into_iter()
        .max()
        .unwrap_or(0)
}

/// 3 列の上端（「作成した設定」の帯の下）
fn columns_top(elastic: Elastic) -> i16 {
    MARGIN + band_height(elastic) + 8
}

/// 詰めものを除いた本体の下端
fn body_bottom(elastic: Elastic) -> i16 {
    columns_top(elastic) + columns_height(elastic)
}

/// 詳細設定を畳んだときのダイアログの高さ
fn folded_height_for(elastic: Elastic) -> i16 {
    body_bottom(elastic) + 8 + BUTTON_HEIGHT + 8 + BUTTON_HEIGHT + MARGIN
}

/// 詳細設定を開いたときのダイアログの高さ
fn expanded_height_for(elastic: Elastic) -> i16 {
    folded_height_for(elastic) + detail_height() + 8
}

/// 中身が画面に入りきらないときにスクロールで見せる
///
/// **入りきるところまで欄を縮める、という作りをやめた代わり。** フォームの行
/// そのものは縮められないので下限があり、1920×1080 の 150% のような
/// 組み合わせでは結局はみ出していた。項目を足すたびに高さの上限とぶつかるのも
/// ここで終わる。
const STYLE_SCROLL: u32 = WS_VSCROLL;

/// メイン画面のテンプレートを組み立てる
pub fn build() -> Template {
    let elastic = Elastic::ideal();
    let mut words: Vec<u16> = Vec::new();
    let mut decor = DETAIL_DECOR_FIRST;

    // 先に高さを出す（見出しに書く必要があるので、項目より前に確定させる）
    let body_bottom = body_bottom(elastic);
    let fold_y = body_bottom + 8;
    let detail_y = fold_y + BUTTON_HEIGHT + 8;
    let detail_height = detail_height();
    let expanded_footer = detail_y + detail_height + 8;

    // 高さの計算は 1 か所（テストもここを見る）
    let folded_height = folded_height_for(elastic);
    let expanded_height = expanded_height_for(elastic);

    // アプリの本体になるので、タスクバーに出て最小化もできる必要がある
    push_header_with(
        &mut words,
        WS_MINIMIZEBOX | STYLE_SCROLL,
        WS_EX_APPWINDOW,
        DIALOG_WIDTH,
        // **作られたときの姿は畳んだ状態。** 開いたぶんは下にはみ出すが、
        // そこはスクロールで見せる
        folded_height,
        "設定づくり ― ExtRun",
    );

    band(&mut words, elastic);
    let top = columns_top(elastic);
    column1(&mut words, top);
    column2(&mut words, top);
    column3(&mut words, top, elastic);

    // --- 折りたたみ ---
    push_item(
        &mut words,
        STYLE_BUTTON,
        col_x(0),
        fold_y,
        110,
        BUTTON_HEIGHT,
        ID_FOLD,
        ATOM_BUTTON,
        "詳細設定を開く ▼",
    );
    push_item(
        &mut words,
        STYLE_STATIC,
        col_x(0) + 116,
        fold_y + 3,
        DIALOG_WIDTH - col_x(0) - 116 - MARGIN,
        LABEL_H,
        u16::MAX,
        ATOM_STATIC,
        "実行前の確認・作業フォルダ・アイコン・管理者・順番・表示する条件・起動の間隔",
    );

    detail(&mut words, detail_y, &mut decor);

    // --- 下の帯 ---
    push_item(
        &mut words,
        STYLE_STATIC,
        col_x(0),
        expanded_footer + 3,
        300,
        LABEL_H,
        ID_FOOTER_NOTE,
        ATOM_STATIC,
        "書いた内容はどこにも保存されません",
    );
    push_item(
        &mut words,
        STYLE_DEFAULT_BUTTON,
        DIALOG_WIDTH - MARGIN - 50,
        expanded_footer,
        50,
        BUTTON_HEIGHT,
        IDCANCEL as u16,
        ATOM_BUTTON,
        "閉じる",
    );

    Template {
        words: to_dword_buffer(&words),
        folded_height,
        expanded_height,
    }
}

// ---------------------------------------------------------------------------
// ① どんな項目にするか
// ---------------------------------------------------------------------------

/// 引数欄の高さ（3 行ぶん）
const ARGS_H: i16 = 30;

fn column1_height() -> i16 {
    GROUP_TOP
        + (LABEL_H + TIGHT + EDIT_H + LOOSE) * 3
        + LABEL_H
        + TIGHT
        + ARGS_H
        + 3
        + EDIT_H
        + 3
        + LABEL_H
        + 5
        + CHECK_H
        + 4
        + CHECK_H
        + 1
        + LABEL_H
        + GROUP_BOTTOM
}

fn column1(words: &mut Vec<u16>, top: i16) {
    let frame = col_x(0);
    let x = frame + GROUP_PAD;
    let mut y = top + GROUP_TOP;

    frame_box(words, frame, top, column1_height(), "① どんな項目にするか");

    label(words, x, &mut y, INNER_W, "メニューに表示する名前");
    edit(words, x, &mut y, INNER_W, ID_NAME);

    // 仕様書と同じ呼び名にする（読み比べるときに引っかからないように）
    label(words, x, &mut y, INNER_W, "アクセスキー（省略可）");
    push_item(words, STYLE_EDIT, x, y, 22, EDIT_H, ID_KEY, ATOM_EDIT, "");
    push_item(
        words,
        STYLE_STATIC,
        x + 28,
        y + 3,
        INNER_W - 28,
        LABEL_H,
        u16::MAX,
        ATOM_STATIC,
        "半角英数字 1 文字。メニュー表示中にこのキーで実行できます",
    );
    y += EDIT_H + LOOSE;

    label(words, x, &mut y, INNER_W, "起動するアプリ");
    push_item(
        words,
        STYLE_EDIT,
        x,
        y,
        INNER_W - 44,
        EDIT_H,
        ID_APP,
        ATOM_EDIT,
        "",
    );
    push_item(
        words,
        STYLE_BUTTON,
        x + INNER_W - 42,
        y,
        42,
        BUTTON_HEIGHT,
        ID_APP_BROWSE,
        ATOM_BUTTON,
        "参照…",
    );
    y += EDIT_H + LOOSE;

    label(words, x, &mut y, INNER_W, "渡す引数");
    push_item(
        words, STYLE_WRAP, x, y, INNER_W, ARGS_H, ID_ARGS, ATOM_EDIT, "",
    );
    y += ARGS_H + 3;

    // 挿入は**選んだ直後ではなくボタンを押したとき**に入れる。
    // CBS_DROPDOWNLIST は矢印キーで辿るたびに CBN_SELCHANGE を飛ばすので、
    // 選択と同時に入れると途中のものが全部挿入される
    push_item(
        words,
        STYLE_COMBOBOX,
        x,
        y,
        INNER_W - 40,
        EDIT_H + 12 * 11,
        ID_INSERT_LIST,
        ATOM_COMBOBOX,
        "",
    );
    push_item(
        words,
        STYLE_BUTTON,
        x + INNER_W - 38,
        y,
        38,
        BUTTON_HEIGHT,
        ID_INSERT,
        ATOM_BUTTON,
        "挿入",
    );
    y += EDIT_H + 3;

    // 早見表を常設せず、**いま書かれているものだけ**の意味を出す
    push_item(
        words,
        STYLE_STATIC,
        x,
        y,
        INNER_W,
        LABEL_H,
        ID_PLACEHOLDER_HINT,
        ATOM_STATIC,
        "",
    );
    y += LABEL_H + 5;

    // 「欄を空にする」は設定ファイルの話で、画面の入力欄と取り違えられる。
    // 仕様書の言い方（引数なしで起動する）に合わせる
    check(words, x, &mut y, INNER_W, ID_NO_ARGS, "引数なしで起動する");
    y += 4;
    // 「1 回でまとめて渡す」だけでは何を渡すのかが読めないので、パスと
    // プロセスを名指しする（仕様書の「すべてまとめて 1 プロセスに渡す」に揃える）
    check(
        words,
        x,
        &mut y,
        INNER_W,
        ID_ALL_MODE,
        "すべてのパスを 1 プロセスにまとめて渡す（+）",
    );
    y += 1;
    hint(words, x, &mut y, INNER_W, "複数選んだときだけ効きます");
}

// ---------------------------------------------------------------------------
// ② どのファイルで表示するか / ③ メニューのどこに表示するか
// ---------------------------------------------------------------------------

fn group2a_height() -> i16 {
    GROUP_TOP
        + LABEL_H
        + TIGHT
        + EDIT_H
        + LOOSE
        + LABEL_H
        + TIGHT
        + EDIT_H
        + 3
        + LABEL_H
        + LOOSE
        + LABEL_H
        + TIGHT
        + CHECK_H
        + 1
        + CHECK_H
        + GROUP_BOTTOM
}

fn group2b_height() -> i16 {
    GROUP_TOP
        + LABEL_H
        + TIGHT
        + EDIT_H
        + LOOSE
        + LABEL_H
        + TIGHT
        + EDIT_H
        + LOOSE
        + CHECK_H
        + GROUP_BOTTOM
}

fn column2_height() -> i16 {
    group2a_height() + 8 + group2b_height()
}

fn column2(words: &mut Vec<u16>, top: i16) {
    let frame = col_x(1);
    let x = frame + GROUP_PAD;
    let mut y = top + GROUP_TOP;

    frame_box(
        words,
        frame,
        top,
        group2a_height(),
        "② どのファイルで表示するか",
    );

    label(words, x, &mut y, INNER_W, "対象の種類");
    push_item(
        words,
        STYLE_COMBOBOX,
        x,
        y,
        INNER_W,
        EDIT_H + 12 * 9,
        ID_EXT_KIND,
        ATOM_COMBOBOX,
        "",
    );
    y += EDIT_H + LOOSE;

    label(words, x, &mut y, INNER_W, "拡張子");
    edit_at(words, x, y, INNER_W, ID_EXT);
    y += EDIT_H + 3;
    hint(
        words,
        x,
        &mut y,
        INNER_W,
        "直に書き替えると、種類は「自分で指定」に変わります",
    );
    y += LOOSE;

    label(words, x, &mut y, INNER_W, "拡張子の書き方");
    push_item(
        words,
        STYLE_RADIO_FIRST,
        x,
        y,
        INNER_W,
        CHECK_H,
        ID_EXT_SECTION,
        ATOM_BUTTON,
        "セクションの見出しにする（[.png .jpg]）",
    );
    y += CHECK_H + 1;
    push_item(
        words,
        STYLE_RADIO,
        x,
        y,
        INNER_W,
        CHECK_H,
        ID_EXT_PERITEM,
        ATOM_BUTTON,
        "項目の行に書く（名前 [.png .jpg] | …）",
    );

    // --- ③ ---
    let third = top + group2a_height() + 8;
    let mut y = third + GROUP_TOP;

    frame_box(
        words,
        frame,
        third,
        group2b_height(),
        "③ メニューのどこに表示するか",
    );

    label(words, x, &mut y, INNER_W, "置き場所");
    push_item(
        words,
        STYLE_COMBOBOX,
        x,
        y,
        INNER_W,
        EDIT_H + 12 * 3,
        ID_PLACE,
        ATOM_COMBOBOX,
        "",
    );
    y += EDIT_H + LOOSE;

    label(
        words,
        x,
        &mut y,
        INNER_W,
        "サブメニューの名前とアクセスキー",
    );
    push_item(
        words,
        STYLE_EDIT,
        x,
        y,
        INNER_W - 26,
        EDIT_H,
        ID_SUB_NAME,
        ATOM_EDIT,
        "",
    );
    push_item(
        words,
        STYLE_EDIT,
        x + INNER_W - 22,
        y,
        22,
        EDIT_H,
        ID_SUB_KEY,
        ATOM_EDIT,
        "",
    );
    y += EDIT_H + LOOSE;

    check(
        words,
        x,
        &mut y,
        INNER_W,
        ID_SEPARATOR,
        "この項目の前に区切り線を入れる（---）",
    );
}

// ---------------------------------------------------------------------------
// 作成した設定（上部に横断する帯）
// ---------------------------------------------------------------------------

/// 帯の幅（ウィンドウいっぱい）
const BAND_W: i16 = DIALOG_WIDTH - MARGIN * 2;
const BAND_INNER: i16 = BAND_W - GROUP_PAD * 2;

fn band_height(elastic: Elastic) -> i16 {
    GROUP_TOP + BUTTON_HEIGHT + 3 + elastic.output + GROUP_BOTTOM
}

/// 貼り先の案内とコピーのボタンを 1 行に並べ、その下に全幅の欄を置く
///
/// **案内とボタンを同じ行にする**のは、どちらも「この文字列をどうするか」の
/// 話で、行を分けるとそのぶん欄が低くなるため。
fn band(words: &mut Vec<u16>, elastic: Elastic) {
    let frame = col_x(0);
    let x = frame + GROUP_PAD;
    let mut y = MARGIN + GROUP_TOP;

    frame_box_wide(
        words,
        frame,
        MARGIN,
        BAND_W,
        band_height(elastic),
        "作成した設定",
    );

    push_item(
        words,
        STYLE_STATIC,
        x,
        y + 3,
        BAND_INNER - 100,
        LABEL_H,
        ID_PASTE_HINT,
        ATOM_STATIC,
        "",
    );
    push_item(
        words,
        STYLE_BUTTON,
        x + BAND_INNER - 90,
        y,
        90,
        BUTTON_HEIGHT,
        ID_COPY,
        ATOM_BUTTON,
        "クリップボードにコピー",
    );
    y += BUTTON_HEIGHT + 3;

    push_item(
        words,
        STYLE_OUTPUT,
        x,
        y,
        BAND_INNER,
        elastic.output,
        ID_OUTPUT,
        ATOM_EDIT,
        "",
    );
}

// ---------------------------------------------------------------------------
// この設定で起動されるもの（3 列目）
// ---------------------------------------------------------------------------

/// プレビューの欄を除いた、3 列目の決まった高さ
const PREVIEW_FIXED: i16 =
    GROUP_TOP + LABEL_H + TIGHT + EDIT_H + LOOSE + LABEL_H + TIGHT + CHECK_H + 4 + GROUP_BOTTOM;

/// 他の列に高さを合わせたときのプレビューの欄の大きさ
///
/// **列の下に死んだ空白を作らない**ためのもの。決め打ちにすると、
/// ①② の下が空くか、3 列目だけが飛び出す。
fn preview_ideal() -> i16 {
    (column1_height().max(column2_height()) - PREVIEW_FIXED).max(Elastic::FLOOR)
}

fn column3_height(elastic: Elastic) -> i16 {
    PREVIEW_FIXED + elastic.preview
}

fn column3(words: &mut Vec<u16>, top: i16, elastic: Elastic) {
    let frame = col_x(2);
    let x = frame + GROUP_PAD;
    let mut y = top + GROUP_TOP;

    frame_box(
        words,
        frame,
        top,
        column3_height(elastic),
        "この設定で起動されるもの",
    );

    label(words, x, &mut y, INNER_W, "試す対象");
    push_item(
        words,
        STYLE_EDIT,
        x,
        y,
        INNER_W - 44,
        EDIT_H,
        ID_TRY_PATH,
        ATOM_EDIT,
        "",
    );
    push_item(
        words,
        STYLE_BUTTON,
        x + INNER_W - 42,
        y,
        42,
        BUTTON_HEIGHT,
        ID_TRY_BROWSE,
        ATOM_BUTTON,
        "参照…",
    );
    y += EDIT_H + LOOSE;

    // 1 つと 3 つを切り替えられるのが肝。`+` や `:when` や `$i` / `$c` の
    // ように、複数選ばないと現れない違いは絵にしないと伝わらない
    label(words, x, &mut y, INNER_W, "選んだ数");
    push_item(
        words,
        STYLE_RADIO_FIRST,
        x,
        y,
        60,
        CHECK_H,
        ID_COUNT_ONE,
        ATOM_BUTTON,
        "1 つ",
    );
    push_item(
        words,
        STYLE_RADIO,
        x + 66,
        y,
        60,
        CHECK_H,
        ID_COUNT_THREE,
        ATOM_BUTTON,
        "3 つ",
    );
    y += CHECK_H + 4;

    push_item(
        words,
        STYLE_OUTPUT,
        x,
        y,
        INNER_W,
        elastic.preview,
        ID_PREVIEW,
        ATOM_EDIT,
        "",
    );
}

// ---------------------------------------------------------------------------
// 詳細設定
// ---------------------------------------------------------------------------

fn detail_height() -> i16 {
    // いちばん高い列（実行のしかた）に合わせる
    12 + CHECK_H
        + 3
        + LABEL_H
        + TIGHT
        + EDIT_H
        + 5
        + LABEL_H
        + 5
        + CHECK_H
        + 4
        + CHECK_H
        + 4
        + CHECK_H
        + 10
}

fn detail(words: &mut Vec<u16>, top: i16, decor: &mut u16) {
    let height = detail_height();

    // --- 実行のしかた ---
    let x = col_x(0);
    group(words, x, top, COL_W, height, decor, "実行のしかた");
    let mut y = top + 12;

    check(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        ID_CONFIRM,
        "実行前に確認する（:confirm）",
    );
    y += 3;
    decor_label(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "確認のメッセージ（省略可）",
    );
    edit_at(words, x + 6, y, COL_W - 12, ID_CONFIRM_MESSAGE);
    y += EDIT_H + 5;
    decor_hint(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "$n などのプレースホルダーが使えます",
    );
    y += 5;

    check(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        ID_ADMIN,
        "管理者として実行する（:admin）",
    );
    y += 4;
    check(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        ID_WAIT,
        "1 つずつ順に実行する（:wait）",
    );
    y += 4;
    check(
        words,
        x + 6,
        &mut y,
        130,
        ID_DELAY,
        "起動の間隔を空ける（:delay）",
    );
    push_item(
        words,
        STYLE_EDIT,
        x + 140,
        y - CHECK_H - 2,
        30,
        EDIT_H,
        ID_DELAY_MS,
        ATOM_EDIT,
        "",
    );
    decor_static(words, x + 174, y - CHECK_H + 1, 40, decor, "ミリ秒");

    // --- 場所と見た目 ---
    let x = col_x(1);
    group(words, x, top, COL_W, height, decor, "場所と見た目");
    let mut y = top + 12;

    decor_label(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "作業フォルダ（:dir）",
    );
    edit_at(words, x + 6, y, COL_W - 12, ID_DIR);
    y += EDIT_H + 3;
    decor_hint(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "省略するとアプリの場所。$d なども書けます",
    );
    y += 5;

    decor_label(words, x + 6, &mut y, COL_W - 12, decor, "アイコン（:icon）");
    edit_at(words, x + 6, y, COL_W - 12 - 44, ID_ICON);
    push_item(
        words,
        STYLE_BUTTON,
        x + 6 + COL_W - 12 - 42,
        y,
        42,
        BUTTON_HEIGHT,
        ID_ICON_PICK,
        ATOM_BUTTON,
        "選ぶ…",
    );
    y += EDIT_H + 3;
    decor_hint(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "「選ぶ…」で一覧から選べます（番号を数えなくてよい）",
    );

    // --- 表示の条件 ---
    let x = col_x(2);
    group(words, x, top, COL_W, height, decor, "表示の条件");
    let mut y = top + 12;

    decor_label(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "メニューに表示する条件（:when）",
    );
    push_item(
        words,
        STYLE_COMBOBOX,
        x + 6,
        y,
        COL_W - 12,
        EDIT_H + 12 * 3,
        ID_WHEN,
        ATOM_COMBOBOX,
        "",
    );
    y += EDIT_H + 3;
    decor_hint(
        words,
        x + 6,
        &mut y,
        COL_W - 12,
        decor,
        "選んだ数で出し分けます",
    );
}

// ---------------------------------------------------------------------------
// 小さな部品
// ---------------------------------------------------------------------------

/// 常に見えているグループ枠（詳細設定のものと違って隠さないので ID は要らない）
fn frame_box(words: &mut Vec<u16>, x: i16, y: i16, height: i16, text: &str) {
    frame_box_wide(words, x, y, COL_W, height, text);
}

fn frame_box_wide(words: &mut Vec<u16>, x: i16, y: i16, width: i16, height: i16, text: &str) {
    push_item(
        words,
        STYLE_GROUP,
        x,
        y,
        width,
        height,
        u16::MAX,
        ATOM_BUTTON,
        text,
    );
}

fn label(words: &mut Vec<u16>, x: i16, y: &mut i16, width: i16, text: &str) {
    push_item(
        words,
        STYLE_STATIC,
        x,
        *y,
        width,
        LABEL_H,
        u16::MAX,
        ATOM_STATIC,
        text,
    );
    *y += LABEL_H + TIGHT;
}

fn hint(words: &mut Vec<u16>, x: i16, y: &mut i16, width: i16, text: &str) {
    push_item(
        words,
        STYLE_STATIC,
        x,
        *y,
        width,
        LABEL_H,
        u16::MAX,
        ATOM_STATIC,
        text,
    );
    *y += LABEL_H;
}

fn edit(words: &mut Vec<u16>, x: i16, y: &mut i16, width: i16, id: u16) {
    edit_at(words, x, *y, width, id);
    *y += EDIT_H + LOOSE;
}

fn edit_at(words: &mut Vec<u16>, x: i16, y: i16, width: i16, id: u16) {
    push_item(words, STYLE_EDIT, x, y, width, EDIT_H, id, ATOM_EDIT, "");
}

fn check(words: &mut Vec<u16>, x: i16, y: &mut i16, width: i16, id: u16, text: &str) {
    push_item(
        words,
        STYLE_CHECK,
        x,
        *y,
        width,
        CHECK_H,
        id,
        ATOM_BUTTON,
        text,
    );
    *y += CHECK_H;
}

/// 詳細設定の中のラベル（隠す必要があるので ID を振る）
fn decor_label(words: &mut Vec<u16>, x: i16, y: &mut i16, width: i16, decor: &mut u16, text: &str) {
    push_item(
        words,
        STYLE_STATIC,
        x,
        *y,
        width,
        LABEL_H,
        *decor,
        ATOM_STATIC,
        text,
    );
    *decor += 1;
    *y += LABEL_H + TIGHT;
}

fn decor_hint(words: &mut Vec<u16>, x: i16, y: &mut i16, width: i16, decor: &mut u16, text: &str) {
    push_item(
        words,
        STYLE_STATIC,
        x,
        *y,
        width,
        LABEL_H,
        *decor,
        ATOM_STATIC,
        text,
    );
    *decor += 1;
    *y += LABEL_H;
}

fn decor_static(words: &mut Vec<u16>, x: i16, y: i16, width: i16, decor: &mut u16, text: &str) {
    push_item(
        words,
        STYLE_STATIC,
        x,
        y,
        width,
        LABEL_H,
        *decor,
        ATOM_STATIC,
        text,
    );
    *decor += 1;
}

fn group(
    words: &mut Vec<u16>,
    x: i16,
    y: i16,
    width: i16,
    height: i16,
    decor: &mut u16,
    text: &str,
) {
    push_item(
        words,
        STYLE_GROUP,
        x,
        y,
        width,
        height,
        *decor,
        ATOM_BUTTON,
        text,
    );
    *decor += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「起動されるもの」の高さは他の列に合わせて決まる（列の下に空白を作らない）
    #[test]
    fn 三列の高さが揃う() {
        let elastic = Elastic::ideal();
        assert_eq!(
            column3_height(elastic),
            column2_height(),
            "② と起動されるものの高さが揃っていない"
        );
        assert!(
            (column2_height() - column1_height()).abs() < 10,
            "① と ② の高さが離れすぎ: {} と {}",
            column1_height(),
            column2_height()
        );
    }

    /// 開いた状態は畳んだ状態より詳細設定のぶんだけ高い
    #[test]
    fn 開くと詳細設定のぶんだけ伸びる() {
        let elastic = Elastic::ideal();
        assert_eq!(
            expanded_height_for(elastic) - folded_height_for(elastic),
            detail_height() + 8
        );
    }
}
