/*!
ツールチップ（欄にマウスを当てたときの説明）

設定できることが多い画面なので、**欄の名前だけでは何を書けばよいか分からない**
ものがある。注記を全部の欄に添えると場所を食うので、短い注記は画面に残し、
それ以上の説明はここに置く。

**このツールだけが `comctl32` を使う。** ツールチップは組み込みクラスでは
作れず、`InitCommonControlsEx` と視覚スタイルのマニフェストが要る
（`build.rs` が `extrun-make.exe` にだけマニフェストを埋めている）。
**`extrun.exe` には入れない** — comctl32 v6 が起動時に読み込まれ、
「起動速度が最優先」に反する。

`TTF_SUBCLASS` を使うので、マウスの出入りはツールチップ側が拾う。
スクロールで欄が動いても付いてくる。
*/

use std::ptr::null_mut;

use extrun::dialog::to_wide;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TTF_IDISHWND, TTF_SUBCLASS,
    TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, GetDlgItem, SendMessageW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::layout::*;

/// ツールチップのクラス名
///
/// `windows-sys` は `TOOLTIPS_CLASS` を文字列定数として出さない。
const TOOLTIPS_CLASS: &str = "tooltips_class32";

/// 折り返す幅（ピクセル）
///
/// 指定しないと 1 行に伸び続けて画面をはみ出す。
const MAX_WIDTH: isize = 340;

/// 欄ごとの説明
///
/// **画面の注記と重ねない。** 欄のすぐ下に出ている短い注記はそのままで、
/// ここには「なぜそれを書くのか」「書かないとどうなるか」を置く。
const TIPS: &[(u16, &str)] = &[
    // --- ① どんな項目にするか ---
    (
        ID_NAME,
        "メニューに並ぶ文字列です。\n記号（& | [ ] @ ^）はそのまま打って構いません — 設定ファイルに書くときに必要なぶんだけ自動で処理します。",
    ),
    (
        ID_KEY,
        "メニューが出ているあいだ、このキーを押すとその項目が実行されます。\n同じ階層で重なると押しても動かなくなるので、extrun.exe --check が重複を警告します。",
    ),
    (
        ID_APP,
        "起動するプログラムのフルパスです。「参照…」で選べます。\n%SystemRoot% のような環境変数と、設定ファイルの別名（@apps など）が使えます。",
    ),
    (
        ID_APP_BROWSE,
        "プログラムを選びます。長いパスを手で打たずに済みます。",
    ),
    (
        ID_ARGS,
        "アプリに渡す引数です。空白で区切ります。\n空白を含めたいところは \"...\" で囲みます。\n空欄のままなら $p（選んだファイルのフルパス）が渡ります。",
    ),
    (
        ID_INSERT_LIST,
        "$p などの書き方を選んで、引数欄のカーソル位置に挿し込めます。\n日時（$t{...}）や入力欄（$?{...}）は、そのまま使える形が並んでいます。",
    ),
    (
        ID_INSERT,
        "選んだ書き方を引数欄に挿し込みます。\n$?{説明} のように打ち替えてほしいところは、挿したあと選択された状態になります。",
    ),
    (
        ID_NO_ARGS,
        "引数をひとつも渡さずに起動します。\n空欄のまま（= $p が渡る）とは意味が違うので、区別できるようにしてあります。",
    ),
    (
        ID_ALL_MODE,
        "複数選んだとき、すべてのパスを 1 プロセスにまとめて渡します。\n付けないと、選んだ数だけプロセスが起動します。\n入力ごとにオプションが要るアプリ（ffmpeg の -i など）には向きません。",
    ),
    // --- ② どの拡張子で表示するか ---
    (
        ID_EXT_KIND,
        "よく使う組み合わせを選ぶと、下の拡張子欄が埋まります。\n設定ファイルに @画像 のような別名があれば、それも並びます。",
    ),
    (
        ID_EXT,
        "この項目を出す拡張子を、先頭のドット付きで空白区切りに並べます。\nfile（フォルダ以外のすべて）と folder だけはドットが要りません。",
    ),
    (
        ID_EXT_STYLE,
        "見出しにすると、その下に書き足す項目にも同じ対象が効きます。\n項目の行に書くと、その項目だけに効きます。\n下の 3 つは貼り先のセクションからの差分なので、貼り先を選ぶ必要があります。",
    ),
    (
        ID_SECTION_SPOT,
        "差分（足す・引く・そのまま）が、どのセクションからの差分なのかを決めます。\nここが決まらないと、この設定で何が起きるかを出せません。",
    ),
    // --- ③ 特殊表示 ---
    (
        ID_SEPARATOR,
        "この項目の前に区切り線（---）を入れます。\n先頭・末尾・連続した区切り線は、メニューを出すときに自動で取り除かれます。",
    ),
    (
        ID_PLACE,
        "メニューの一番上に置くか、サブメニューの中に入れるかを決めます。\n設定ファイルにサブメニューがあれば、その中に足すことも選べます。",
    ),
    (
        ID_SUB_NAME,
        "新しく作るサブメニューの名前です。\n設定ファイルに同じ名前のサブメニューが既にあると、同じ名前のものが 2 つ並びます。",
    ),
    // --- 作成した設定 ---
    (
        ID_OUTPUT,
        "この文字列をそのまま設定ファイルに貼り付けます。\nCtrl+A で全部選べます。",
    ),
    (ID_COPY, "作成した設定をクリップボードに入れます。"),
    (
        ID_OPEN_CONFIG,
        "extrun-config.txt を、txt に関連付けられたアプリで開きます。\nコピー → 開く → 貼り付け、の流れで使えます。",
    ),
    // --- 起動されるもの ---
    (
        ID_TRY_PATH,
        "この設定を試す対象のパスです。実在しなくても構いません。\nここの拡張子が、上で指定した対象に合っているかどうかも見ます。",
    ),
    (ID_COUNT_ONE, "1 つだけ選んだときの動きを見ます。"),
    (
        ID_COUNT_THREE,
        "3 つ選んだときの動きを見ます。\n+（まとめて渡す）・:when・$i / $c・:wait のように、複数選ばないと現れない違いはここで確かめられます。",
    ),
    (
        ID_PREVIEW,
        "上の設定で実際に起動されるコマンドラインです。\n書き方に問題があれば、extrun.exe --check と同じ理由がここに出ます。",
    ),
    // --- 詳細設定 ---
    (
        ID_CONFIRM,
        "実行する前に確認のダイアログを出します。既定のボタンは「いいえ」です。",
    ),
    (
        ID_CONFIRM_MESSAGE,
        "確認のダイアログに添える文章です。$n（ファイル名）や $c（総数）が使えます。",
    ),
    (
        ID_ADMIN,
        "管理者として実行します（UAC の確認が出ます）。\n昇格はプロセスごとなので、複数選んで個別に起動すると対象の数だけ確認が出ます。",
    ),
    (
        ID_WAIT,
        "前の 1 つが終了してから次を起動します。\n開く → 手で直す → 閉じると次が開く、という使い方ができます。\n起動してすぐ終わるアプリ（単一インスタンス型）では効きません。",
    ),
    (
        ID_DELAY,
        "1 つずつの起動のあいだに間を空けます。\n同時に実行する数を減らすものではなく、起動が重なることによる取りこぼしを防ぐためのものです。",
    ),
    (ID_DELAY_MS, "空ける時間（ミリ秒）。10〜10000 の範囲です。"),
    (
        ID_WHEN,
        "選んだ数によって、メニューに出すかどうかを変えます。",
    ),
    (
        ID_DIR,
        "アプリを動かすフォルダです。省略するとアプリが置かれている場所になります。\n$d（親フォルダ）などが使えます。",
    ),
    (
        ID_ICON,
        "メニューの名前の左に出すアイコンです。「選ぶ…」で一覧から選べます。",
    ),
    (
        ID_ICON_PICK,
        "アイコンを一覧から選びます。番号を数える必要がありません。",
    ),
    (
        ID_ALIAS,
        "設定ファイルに書かれている別名です。長いパスを @名前 と書けます。",
    ),
    (
        ID_ALIAS_INSERT,
        "選んだ別名を、最後に触っていた欄のカーソル位置に挿し込みます。",
    ),
];

/// ダイアログの各欄にツールチップを付ける
///
/// **入力欄の差し替え（`edits::enable_select_all`）より後に呼ぶ。**
/// `TTF_SUBCLASS` も手続きを差し替えるので、順番が逆だと片方が失われる。
pub fn attach(dialog: HWND) {
    unsafe {
        let init = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&init);

        let class = to_wide(TOOLTIPS_CLASS);
        let tip = CreateWindowExW(
            WS_EX_TOPMOST,
            class.as_ptr(),
            null_mut(),
            WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            dialog,
            null_mut(),
            GetModuleHandleW(null_mut()),
            null_mut(),
        );
        if tip.is_null() {
            // 出なくても画面は使える。黙って諦める
            return;
        }

        // 指定しないと 1 行に伸び続けて画面をはみ出す
        SendMessageW(tip, TTM_SETMAXTIPWIDTH, 0, MAX_WIDTH);

        for (id, text) in TIPS {
            let control = GetDlgItem(dialog, *id as i32);
            if control.is_null() {
                continue;
            }

            let mut wide = to_wide(text);
            let mut info: TTTOOLINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
            info.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
            info.hwnd = dialog;
            info.uId = control as usize;
            info.lpszText = wide.as_mut_ptr();

            SendMessageW(tip, TTM_ADDTOOLW, 0, &info as *const TTTOOLINFOW as isize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同じ欄に 2 つ書くと、あとから足したほうが黙って無視される
    #[test]
    fn 説明は欄ごとに一つ() {
        let mut ids: Vec<u16> = TIPS.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(ids, unique, "同じ欄に説明が 2 つある");
    }

    /// 空の説明を付けると、当たっても何も出ない欄ができる
    #[test]
    fn 説明は空でない() {
        for (id, text) in TIPS {
            assert!(!text.trim().is_empty(), "{} の説明が空", id);
        }
    }
}
