/*!
起動の進行状況ダイアログ（`:delay`）

`:delay` で間隔を空けて起動するとき、待っている間 ExtRun は無表示のまま
生き続けることになる。何も出さないと「終わったのか止まったのか分からず、
やめることもできない」ので、合計が長くなる場合だけこのダイアログを出す。

**待ち時間の実体はこのダイアログのモーダルループ**。`thread::sleep` で眠ると
メッセージを汲めなくなり、中止ボタンも再描画も効かなくなる。`SetTimer` を
仕掛けて `WM_TIMER` のたびに 1 つずつ起動すれば、待っている間もプロセスは
応答したままでいられる。

`WM_TIMER` は低優先度で合体もされるので、間隔は「ちょうど N ミリ秒」ではなく
**「最低でも N ミリ秒」**になる。起動の競合を避けるという目的にはそれで足りる。

中止できるのは**まだ起動していない残り**だけで、起動済みのプロセスは止まらない
（ExtRun は起動したプロセスのハンドルを持たない）。取り違えると危ないので、
ボタンの名前と本文の両方でそう書く。UAC を断ったときの「残りを起動しない」と
同じ意味で、実装も揃えてある。
*/

use crate::dialog::{
    self, push_header, push_item, to_dword_buffer, to_wide, ATOM_BUTTON, ATOM_STATIC,
    BUTTON_HEIGHT, BUTTON_WIDTH, MARGIN, STYLE_BUTTON, STYLE_DEFAULT_BUTTON, STYLE_STATIC,
};
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// クリップボードの書式（`windows-sys` が `Win32_System_Ole` を有効にしないと出さない）
const CF_UNICODETEXT: u32 = 13;

/// コントロール ID（`IDOK` = 1 / `IDCANCEL` = 2 と重ならない値）
const ID_STATUS: u16 = 100;
const ID_PAUSE: u16 = 101;
const ID_COPY: u16 = 102;

/// 起動の間隔を刻むタイマー
const TIMER_ID: usize = 1;

/// ダイアログの大きさ（ダイアログ単位。フォントに合わせて拡大縮小される）
const DIALOG_WIDTH: i16 = 260;
const LINE_HEIGHT: i16 = 10;
const LINE_GAP: i16 = 3;
const WIDE_BUTTON_WIDTH: i16 = 96;

/// 進行状況ダイアログを出したあとの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 最後まで起動した
    Finished,
    /// 「以降の起動を取り消す」が押された
    Cancelled,
    /// 起動の側から打ち切られた（UAC を断られた）
    Stopped,
    /// ダイアログを出せなかった（呼び出し側は間隔だけ空けて起動する）
    Failed,
}

/// ダイアログとやり取りする値
struct ProgressData<'a> {
    /// 見出しに出す項目の名前
    name: String,
    /// 起動する総数
    total: usize,
    /// 起動と起動の間隔（ミリ秒）
    delay: u32,
    /// 次に起動する番号
    next: usize,
    /// 一時停止中か
    paused: bool,
    /// 起動の側から打ち切られたか
    stopped: bool,
    /// 1 つ起動する。`false` を返すと以降を起動しない
    launch: &'a mut dyn FnMut(usize) -> bool,
}

/// 進行状況を見せながら、間隔を空けて順に起動する
///
/// `launch` は番号を受け取って 1 つ起動し、続けてよいかを返す。起動そのものを
/// このモジュールが知らないのは、組み立てを `menu::resolve_invocations` の
/// 1 か所に集めているため。
pub fn run(name: &str, total: usize, delay: u32, launch: &mut dyn FnMut(usize) -> bool) -> Outcome {
    let template = build_template();
    let mut data = ProgressData {
        name: name.to_string(),
        total,
        delay,
        next: 0,
        paused: false,
        stopped: false,
        launch,
    };

    let selected = dialog::show_modal(
        &template,
        Some(dialog_proc),
        &mut data as *mut ProgressData as LPARAM,
    );

    // 組み立てを誤ると -1 が返る。ここで黙って諦めると「選んだのに何も
    // 起きない」になるので、呼び出し側が間隔だけ空けて起動できるようにする
    if selected == -1 {
        return Outcome::Failed;
    }

    if data.stopped {
        Outcome::Stopped
    } else if selected == IDCANCEL as isize {
        Outcome::Cancelled
    } else {
        Outcome::Finished
    }
}

/// ダイアログテンプレートを組み立てる
fn build_template() -> Vec<u32> {
    let mut words: Vec<u16> = Vec::new();

    let status_y = MARGIN + LINE_HEIGHT + LINE_GAP;
    let note_y = status_y + LINE_HEIGHT + LINE_GAP;
    let button_y = note_y + LINE_HEIGHT + MARGIN;
    let dialog_height = button_y + BUTTON_HEIGHT + MARGIN;
    let content_width = DIALOG_WIDTH - MARGIN * 2;

    push_header(&mut words, 5, DIALOG_WIDTH, dialog_height);

    // --- 何を起動しているか（項目名） ---
    push_item(
        &mut words,
        STYLE_STATIC,
        MARGIN,
        MARGIN,
        content_width,
        LINE_HEIGHT,
        u16::MAX, // WM_INITDIALOG で一度入れるだけなので ID は不要
        ATOM_STATIC,
        "",
    );

    // --- 何件まで進んだか（起動のたびに書き換える） ---
    push_item(
        &mut words,
        STYLE_STATIC,
        MARGIN,
        status_y,
        content_width,
        LINE_HEIGHT,
        ID_STATUS,
        ATOM_STATIC,
        "",
    );

    // --- 中止の意味 ---
    //
    // 押したあとに知らせても遅い。押す前に見えている必要がある
    push_item(
        &mut words,
        STYLE_STATIC,
        MARGIN,
        note_y,
        content_width,
        LINE_HEIGHT,
        u16::MAX,
        ATOM_STATIC,
        "中止できるのはまだ起動していない分だけです。",
    );

    // --- 一時停止 / 中止 ---
    push_item(
        &mut words,
        STYLE_DEFAULT_BUTTON,
        DIALOG_WIDTH - MARGIN - WIDE_BUTTON_WIDTH - BUTTON_WIDTH - 4,
        button_y,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
        ID_PAUSE,
        ATOM_BUTTON,
        "一時停止",
    );
    push_item(
        &mut words,
        STYLE_BUTTON,
        DIALOG_WIDTH - MARGIN - WIDE_BUTTON_WIDTH,
        button_y,
        WIDE_BUTTON_WIDTH,
        BUTTON_HEIGHT,
        IDCANCEL as u16,
        ATOM_BUTTON,
        "以降の起動を取り消す",
    );

    to_dword_buffer(&words)
}

/// ダイアログプロシージャ
unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_INITDIALOG => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam);
            let data = &mut *(lparam as *mut ProgressData);

            set_text(hwnd, u16::MAX, &format!("{} を起動しています", data.name));

            // 1 つ目は待たずに起動する。押した直後に何も起きないと、選び直したく
            // なって二重に起動される。待つのは起動と起動の「あいだ」
            if step(hwnd, data) {
                SetTimer(hwnd, TIMER_ID, data.delay, None);
            }

            1
        }

        WM_TIMER => {
            if wparam != TIMER_ID {
                return 0;
            }
            if let Some(data) = data_of(hwnd) {
                step(hwnd, data);
            }
            1
        }

        WM_COMMAND => {
            let control = (wparam & 0xFFFF) as u16;
            let Some(data) = data_of(hwnd) else {
                return 0;
            };

            match control {
                ID_PAUSE => {
                    toggle_pause(hwnd, data);
                    1
                }
                id if id == IDCANCEL as u16 => {
                    stop_timer(hwnd);
                    EndDialog(hwnd, IDCANCEL as isize);
                    1
                }
                _ => 0,
            }
        }

        // × で閉じたときも「以降の起動を取り消す」と同じ扱いにする
        WM_CLOSE => {
            stop_timer(hwnd);
            EndDialog(hwnd, IDCANCEL as isize);
            1
        }

        _ => 0,
    }
}

/// `WM_INITDIALOG` で控えたポインタを取り出す
unsafe fn data_of<'a>(hwnd: HWND) -> Option<&'a mut ProgressData<'a>> {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ProgressData;
    if pointer.is_null() {
        None
    } else {
        Some(&mut *pointer)
    }
}

/// 次の 1 つを起動する（続けるなら `true`）
///
/// 最後まで行ったか打ち切られたときは、ここでタイマーを止めてダイアログを閉じる。
unsafe fn step(hwnd: HWND, data: &mut ProgressData) -> bool {
    let index = data.next;
    let keep_going = (data.launch)(index);
    data.next = index + 1;
    update_status(hwnd, data);

    if !keep_going {
        data.stopped = true;
    }

    if !keep_going || data.next >= data.total {
        stop_timer(hwnd);
        EndDialog(hwnd, IDOK as isize);
        return false;
    }

    true
}

/// 一時停止と再開を切り替える
///
/// 再開したらすぐ次を起動する。間隔ぶん待たせると、押しても何も起きない時間が
/// できて壊れているように見える。
unsafe fn toggle_pause(hwnd: HWND, data: &mut ProgressData) {
    data.paused = !data.paused;

    if data.paused {
        stop_timer(hwnd);
        set_text(hwnd, ID_PAUSE, "再開");
        update_status(hwnd, data);
        return;
    }

    set_text(hwnd, ID_PAUSE, "一時停止");
    if step(hwnd, data) {
        SetTimer(hwnd, TIMER_ID, data.delay, None);
    }
}

/// 進み具合の行を書き換える
unsafe fn update_status(hwnd: HWND, data: &ProgressData) {
    let paused = if data.paused {
        "（一時停止中）"
    } else {
        ""
    };
    set_text(
        hwnd,
        ID_STATUS,
        &format!("{} / {} を起動しました{}", data.next, data.total, paused),
    );
}

unsafe fn stop_timer(hwnd: HWND) {
    KillTimer(hwnd, TIMER_ID);
}

unsafe fn set_text(hwnd: HWND, id: u16, text: &str) {
    SetDlgItemTextW(hwnd, id as i32, to_wide(text).as_ptr());
}

// =====================================================================
// 中止したあとの要約
// =====================================================================

/// 要約ダイアログとやり取りする値
struct SummaryData {
    /// クリップボードに渡す本文（残りのパスを 1 行ずつ）
    paths: String,
}

/// 途中で止まったときに、何を起動して何が残ったかを見せる
///
/// **残りをファイルに書き出さない**のは、書ける場所を ExtRun が決められない
/// ため（exe の場所は書き込めるとは限らず、`%TEMP%` は見つけられず、
/// `%LOCALAPPDATA%` は消す手段が無い）。クリップボードに渡せば、貼り先も
/// 保存するかどうかも使う人が決められる。「ExtRun は実行中にファイルを
/// 書き出さない」という約束もそのまま残る。
pub fn show_summary(outcome: Outcome, started: usize, total: usize, remaining: &[String]) {
    let reason = match outcome {
        Outcome::Cancelled => "以降の起動を取り消しました。",
        _ => "起動を中断しました。",
    };

    let lines = [
        reason.to_string(),
        format!(
            "{} 件中 {} 件を起動しました（起動済みのものは停止していません）。",
            total, started
        ),
        format!("残りの {} 件は起動していません。", remaining.len()),
    ];

    // 貼り付け先で 1 行ずつになるように CRLF で繋ぐ
    let mut paths = remaining.join("\r\n");
    if !paths.is_empty() {
        paths.push_str("\r\n");
    }

    let template = build_summary_template(&lines, !paths.is_empty());
    let mut data = SummaryData { paths };

    let selected = dialog::show_modal(
        &template,
        Some(summary_proc),
        &mut data as *mut SummaryData as LPARAM,
    );

    // 要約すら出せないときは、せめて同じことを伝える
    if selected == -1 {
        crate::show_error_dialog("ExtRun", &lines.join("\n"));
    }
}

/// 要約ダイアログのテンプレートを組み立てる
fn build_summary_template(lines: &[String; 3], has_paths: bool) -> Vec<u32> {
    let mut words: Vec<u16> = Vec::new();

    let button_y = MARGIN + (LINE_HEIGHT + LINE_GAP) * lines.len() as i16 + MARGIN - LINE_GAP;
    let dialog_height = button_y + BUTTON_HEIGHT + MARGIN;
    let content_width = DIALOG_WIDTH - MARGIN * 2;
    let item_count = lines.len() as u16 + if has_paths { 2 } else { 1 };

    push_header(&mut words, item_count, DIALOG_WIDTH, dialog_height);

    for (index, line) in lines.iter().enumerate() {
        push_item(
            &mut words,
            STYLE_STATIC,
            MARGIN,
            MARGIN + (LINE_HEIGHT + LINE_GAP) * index as i16,
            content_width,
            LINE_HEIGHT,
            u16::MAX,
            ATOM_STATIC,
            line,
        );
    }

    // **「閉じる」を先に書く**。最初のタブストップになるのは書いた順で、位置では
    // ないので、これで Enter が閉じるほうに効く（コピーが先だと、閉じたつもりの
    // Enter でコピーが走る）。並びは x 座標で決まるので見た目は変わらない
    push_item(
        &mut words,
        STYLE_DEFAULT_BUTTON,
        DIALOG_WIDTH - MARGIN - BUTTON_WIDTH,
        button_y,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
        IDOK as u16,
        ATOM_BUTTON,
        "閉じる",
    );

    // 残りが無いときにコピーのボタンを出すと、押しても何も入らない
    if has_paths {
        push_item(
            &mut words,
            STYLE_BUTTON,
            DIALOG_WIDTH - MARGIN - WIDE_BUTTON_WIDTH - BUTTON_WIDTH - 4,
            button_y,
            WIDE_BUTTON_WIDTH,
            BUTTON_HEIGHT,
            ID_COPY,
            ATOM_BUTTON,
            "残りのパスをコピー",
        );
    }

    to_dword_buffer(&words)
}

/// 要約ダイアログのプロシージャ
unsafe extern "system" fn summary_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_INITDIALOG => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam);
            1
        }

        WM_COMMAND => {
            let control = (wparam & 0xFFFF) as u16;

            if control == ID_COPY {
                let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const SummaryData;
                if !pointer.is_null() {
                    let copied = copy_to_clipboard(hwnd, &(*pointer).paths);
                    // 押した結果が見えないと、コピーされたのか分からない
                    set_text(
                        hwnd,
                        ID_COPY,
                        if copied {
                            "コピーしました"
                        } else {
                            "コピーできませんでした"
                        },
                    );
                }
                return 1;
            }

            if control == IDOK as u16 || control == IDCANCEL as u16 {
                EndDialog(hwnd, control as isize);
                return 1;
            }

            0
        }

        _ => 0,
    }
}

/// 文字列をクリップボードに置く
///
/// 置けたらクリップボードが確保した領域の持ち主になるので、こちらでは解放しない。
/// 失敗したぶんは解放しないまま残るが、ExtRun はこの直後に終了するので実害は無い
/// （`GlobalFree` はこの用途のためだけに `windows-sys` の定義を増やすことになる）。
fn copy_to_clipboard(hwnd: HWND, text: &str) -> bool {
    let wide = to_wide(text);
    let bytes = std::mem::size_of_val(&wide[..]);

    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return false;
        }

        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            CloseClipboard();
            return false;
        }

        let buffer = GlobalLock(handle) as *mut u16;
        if buffer.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), buffer, wide.len());
        GlobalUnlock(handle);

        // 置き換えるので、書き込みに成功してから空にする
        EmptyClipboard();
        let placed = !SetClipboardData(CF_UNICODETEXT, handle as HANDLE).is_null();
        CloseClipboard();

        placed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindow, PostMessageW};

    /// 実際にダイアログを出して確かめる
    ///
    /// テンプレートの組み立てはコンパイラが検査できないので、実機で 1 度は
    /// 通しておきたい。ただし画面が要るため既定では走らせない。
    /// **`cargo test -- --ignored` で実行する。**
    ///
    /// `prompt.rs` の実機テストと同じく、ダイアログを題名で探すので 1 つの
    /// テストにまとめてある（並行して走ると互いのダイアログを掴む）。
    #[test]
    #[ignore = "画面が必要（cargo test -- --ignored で実行）"]
    fn 進行状況ダイアログの操作をひととおり確かめる() {
        // --- 最後まで起動する ---
        static 回数: AtomicUsize = AtomicUsize::new(0);
        回数.store(0, Ordering::SeqCst);

        let mut 起動 = |_index: usize| {
            回数.fetch_add(1, Ordering::SeqCst);
            true
        };
        assert_eq!(
            run("テスト", 3, 10, &mut 起動),
            Outcome::Finished,
            "打ち切られなければ Finished になる"
        );
        assert_eq!(回数.load(Ordering::SeqCst), 3, "総数だけ起動される");

        // --- 中止すると残りは起動されない ---
        回数.store(0, Ordering::SeqCst);
        let 操作 = std::thread::spawn(押して閉じる);
        let mut 起動 = |_index: usize| {
            回数.fetch_add(1, Ordering::SeqCst);
            true
        };
        assert_eq!(run("テスト", 50, 300, &mut 起動), Outcome::Cancelled);
        操作.join().expect("操作のスレッドが終わる");
        assert!(
            回数.load(Ordering::SeqCst) < 50,
            "中止したので全部は起動されない"
        );

        // --- 一時停止しているあいだは進まない ---
        回数.store(0, Ordering::SeqCst);
        let 操作 = std::thread::spawn(|| {
            let hwnd = ダイアログを待つ().expect("進行状況ダイアログが出る");

            unsafe { PostMessageW(hwnd, WM_COMMAND, ID_PAUSE as WPARAM, 0) };
            std::thread::sleep(std::time::Duration::from_millis(300));
            let 止めた時 = 回数.load(Ordering::SeqCst);

            std::thread::sleep(std::time::Duration::from_millis(400));
            assert_eq!(
                回数.load(Ordering::SeqCst),
                止めた時,
                "一時停止中はタイマーが止まるので増えない"
            );

            // 再開したら、間隔を待たずにすぐ次が起動する
            unsafe { PostMessageW(hwnd, WM_COMMAND, ID_PAUSE as WPARAM, 0) };
            std::thread::sleep(std::time::Duration::from_millis(150));
            assert!(回数.load(Ordering::SeqCst) > 止めた時, "再開したら進む");

            unsafe { PostMessageW(hwnd, WM_COMMAND, IDCANCEL as WPARAM, 0) };
            閉じるまで待つ(hwnd);
        });
        let mut 起動 = |_index: usize| {
            回数.fetch_add(1, Ordering::SeqCst);
            true
        };
        assert_eq!(run("テスト", 50, 200, &mut 起動), Outcome::Cancelled);
        操作.join().expect("一時停止の確認が通る");

        // --- 起動の側から打ち切ると Stopped になる ---
        let mut 起動 = |index: usize| index < 1;
        assert_eq!(
            run("テスト", 10, 10, &mut 起動),
            Outcome::Stopped,
            "launch が false を返したら以降は起動しない"
        );

        // --- 要約が出せる（残りありと残りなしの両方） ---
        let 操作 = std::thread::spawn(押して閉じる);
        show_summary(Outcome::Cancelled, 2, 5, &["C:\\a.txt".to_string()]);
        操作.join().expect("操作のスレッドが終わる");

        let 操作 = std::thread::spawn(押して閉じる);
        show_summary(Outcome::Stopped, 5, 5, &[]);
        操作.join().expect("操作のスレッドが終わる");
    }

    /// ダイアログが現れたらキャンセルを送って閉じる
    fn 押して閉じる() {
        if let Some(hwnd) = ダイアログを待つ() {
            unsafe { PostMessageW(hwnd, WM_COMMAND, IDCANCEL as WPARAM, 0) };
            閉じるまで待つ(hwnd);
        }
    }

    /// ダイアログが現れるまで待つ（題名で探す）
    fn ダイアログを待つ() -> Option<HWND> {
        let 期限 = Instant::now() + std::time::Duration::from_secs(5);
        while Instant::now() < 期限 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let hwnd = unsafe { FindWindowW(null_mut(), to_wide(dialog::TITLE).as_ptr()) };
            if !hwnd.is_null() {
                return Some(hwnd);
            }
        }
        None
    }

    fn 閉じるまで待つ(hwnd: HWND) {
        let 期限 = Instant::now() + std::time::Duration::from_secs(5);
        while unsafe { IsWindow(hwnd) } != 0 && Instant::now() < 期限 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
