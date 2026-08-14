/*!
ビルド設定

- リリースビルドではコンソールを持たない（`/SUBSYSTEM:WINDOWS`）
- exe に VERSIONINFO を埋め込む（プロパティの「詳細」タブに出る情報）

VERSIONINFO のバージョンは Cargo.toml から取るので、ここには書かない。
リソーススクリプト（.rc）は OUT_DIR に生成する。リポジトリに .rc を置くと
バージョンの二重管理になるため。
*/

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    #[cfg(windows)]
    {
        // リリースビルドの場合のみコンソールを非表示
        if env::var("PROFILE").unwrap() == "release" {
            println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
            println!("cargo:rustc-link-arg=/ENTRY:mainCRTStartup");
        }

        embed_version_info();
    }
}

/// VERSIONINFO を生成して exe に埋め込む
#[cfg(windows)]
fn embed_version_info() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR がある"));
    let rc_path = out_dir.join("extrun.rc");

    fs::write(&rc_path, version_info_rc()).expect("リソーススクリプトを書き出せる");

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("リソースをコンパイルできる（Windows SDK の rc.exe が必要）");
}

/// リソーススクリプトの中身を組み立てる
///
/// `#pragma code_page(65001)` を先頭に置いて UTF-8 として読ませる。言語 ID の
/// 0x0411 は日本語、コードページの 1200 は Unicode で、ブロック名の
/// `041104B0` はその 2 つを並べたもの。
#[cfg(windows)]
fn version_info_rc() -> String {
    let major = env::var("CARGO_PKG_VERSION_MAJOR").expect("メジャーバージョンがある");
    let minor = env::var("CARGO_PKG_VERSION_MINOR").expect("マイナーバージョンがある");
    let patch = env::var("CARGO_PKG_VERSION_PATCH").expect("パッチバージョンがある");
    let version = env::var("CARGO_PKG_VERSION").expect("バージョンがある");

    format!(
        r#"#pragma code_page(65001)

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004L
FILETYPE 0x1L
{{
    BLOCK "StringFileInfo"
    {{
        BLOCK "041104B0"
        {{
            VALUE "CompanyName", "pirukabocha"
            VALUE "FileDescription", "ExtRun - 拡張子ごとのメニューから開くランチャー"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "extrun"
            VALUE "LegalCopyright", "Copyright (c) 2025-2026 pirukabocha"
            VALUE "OriginalFilename", "extrun.exe"
            VALUE "ProductName", "ExtRun"
            VALUE "ProductVersion", "{version}"
        }}
    }}
    BLOCK "VarFileInfo"
    {{
        VALUE "Translation", 0x411, 1200
    }}
}}
"#
    )
}
