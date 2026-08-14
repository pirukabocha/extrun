/*!
コンソールへの出力

本体は /SUBSYSTEM:WINDOWS でビルドされるため、そのままでは標準出力がどこにも
繋がらない。`--check` のときだけ次の順で出力先を確保する。

1. 標準出力ハンドル（コンソールから起動された場合やリダイレクトされた場合）
2. 親プロセスのコンソールに `AttachConsole` して `CONOUT$` を開く
3. どちらも失敗したらメッセージダイアログ
*/

use std::iter::once;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};

/// 出力先のハンドルを確保する
fn acquire_handle() -> Option<HANDLE> {
    unsafe {
        // コンソールから起動された場合やリダイレクトされている場合はそのまま使う
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            return Some(handle);
        }

        // 親プロセスのコンソールに接続する。Rust の println! は AttachConsole
        // だけでは出力されないため、CONOUT$ を直接開いて書き込む。
        AttachConsole(ATTACH_PARENT_PROCESS);

        let name: Vec<u16> = "CONOUT$".encode_utf16().chain(once(0)).collect();
        let handle = CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );

        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            Some(handle)
        } else {
            None
        }
    }
}

/// コンソールへ文字列を出力する（確保できなければダイアログ表示）
pub fn print(text: &str) {
    let Some(handle) = acquire_handle() else {
        crate::show_error_dialog("ExtRun --check", text);
        return;
    };

    // コンソールには UTF-16 で書く（コードページの影響を受けない）。
    // WriteConsoleW はパイプやファイルのハンドルでは失敗するので、
    // その結果でリダイレクトを判定する（GetConsoleMode はハンドルのアクセス権に
    // 左右されるため、書き込み専用ハンドルでは判定に使えない）。
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut written = 0u32;
    let ok = unsafe {
        WriteConsoleW(
            handle,
            wide.as_ptr(),
            wide.len() as u32,
            &mut written,
            null_mut(),
        )
    } != 0;

    if !ok {
        // ファイルやパイプへのリダイレクトは UTF-8 で書く
        let bytes = text.as_bytes();
        let mut written = 0u32;
        unsafe {
            WriteFile(
                handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                null_mut(),
            );
        }
    }
}
