/*!
ExtRun の中身

`extrun.exe`（`main.rs`）はこのライブラリの薄い入口で、コマンドライン引数を
読んで、ここに置いてあるモジュールを呼ぶだけになっている。

**ライブラリに切り出してあるのは、設定ファイルを扱う道具を 2 つ目の実行ファイル
から使うため。** 設定ファイルの数行を組み立てる GUI（`extrun-make`）が、
エスケープの規則（`text.rs`）と、起動されるコマンドラインの組み立て
（`invoke::resolve_invocations`）を**同じ実装のまま**呼べる必要がある。
書き直すと、CLAUDE.md に記録してある「`SPECIALS` が 2 か所に分かれて片方だけ
直す事故」を、今度はプロセスをまたいで再現することになる。

## 公開の範囲

`pub mod` にしてあるのは**外から実際に呼ばれているものだけ**で、残りは
`pub(crate) mod` に留めてある。全部を `pub` にすると、使われなくなった関数への
`dead_code` 警告が出なくなり、消し忘れが静かに残るため。必要になった時点で
1 つずつ開ける（開けるときは、なぜ外から要るのかを添えること）。
*/

pub mod check;
pub mod config;
pub mod console;
pub mod menu;
pub mod preview;

// --- extrun-make（設定づくり）のために開けたもの ---
/// `DLGTEMPLATE` の組み立て。3 列のメイン画面とアイコン選択の画面が使う。
/// 組み立ての知識を 2 か所に分けないため（`dialog.rs` の冒頭を参照）。
pub mod dialog;
/// エスケープを**掛ける**方向（`escape_name` / `escape_path`）を使う。
/// 自前で書くと `SPECIALS` が 2 実装に分かれる。
pub mod text;

pub(crate) mod confirm;
pub(crate) mod datetime;
pub(crate) mod filter;
pub(crate) mod icon;
pub(crate) mod invoke;
pub(crate) mod launch;
pub(crate) mod placeholder;
pub(crate) mod progress;
pub(crate) mod prompt;

use std::path::PathBuf;

/// ターゲットファイル/フォルダの情報
#[derive(Debug, Clone)]
pub struct Target {
    /// ファイルタイプ（`folder` / `.txt` のような拡張子 / `file`）
    pub file_type: String,
    /// ファイルパス
    pub path: PathBuf,
}

impl Target {
    /// パスからターゲットを作成
    pub fn from_path(path: PathBuf) -> Self {
        let file_type = if path.is_dir() {
            "folder".to_string()
        } else {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| format!(".{}", ext.to_lowercase()))
                .unwrap_or_else(|| "file".to_string())
        };

        Target { file_type, path }
    }
}

/// エラーダイアログを表示
///
/// リリースビルドにはコンソールが無いので、`eprintln!` はどこにも出ない。
/// ユーザーに何かを伝える手段はこれと `console.rs` の 2 つだけ。
pub fn show_error_dialog(title: &str, message: &str) {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};

    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(once(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(once(0)).collect();

    unsafe {
        MessageBoxW(
            null_mut(),
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_OK | MB_ICONWARNING,
        );
    }
}
