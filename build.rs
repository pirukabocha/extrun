/*!
ビルド設定

- リリースビルドではコンソールを持たない（`/SUBSYSTEM:WINDOWS`）
- exe に VERSIONINFO を埋め込む（プロパティの「詳細」タブに出る情報）
- `extrun-make.exe` にだけ、視覚スタイルのマニフェストを埋め込む

VERSIONINFO のバージョンは Cargo.toml から取るので、ここには書かない。
リソーススクリプト（.rc）は OUT_DIR に生成する。リポジトリに .rc を置くと
バージョンの二重管理になるため。

**exe ごとに別のリソースを埋め込む。** `embed_resource::compile` は
すべての bin にリンクするので、そのままだと `extrun-make.exe` のプロパティに
`OriginalFilename: extrun.exe` が入る。`compile_for` で振り分ける。
*/

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

        embed_resources();
    }
}

/// exe ごとのリソースを生成して埋め込む
#[cfg(windows)]
fn embed_resources() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR がある"));

    // --- extrun.exe ---
    //
    // **マニフェストは埋めない。** 視覚スタイルを有効にすると comctl32 v6 が
    // アクティベーションコンテキストごと起動時に読み込まれる。ExtRun は
    // 起動速度が最優先で、メニューはマニフェストが無くてもテーマに従うので、
    // 払う理由が無い。
    write_and_compile(
        &out_dir,
        "extrun.rc",
        &version_info_rc(
            "ExtRun - 拡張子ごとのメニューから開くランチャー",
            "extrun",
            "extrun.exe",
            None,
        ),
        "extrun",
    );

    // --- extrun-make.exe ---
    //
    // こちらは**マニフェストを埋める**。ツールチップに comctl32 が要るうえ、
    // 無いままだと入力欄もボタンもクラシックな見た目になる。常駐せず、
    // ユーザーが明示的に起動する道具なので、読み込みのぶんは払ってよい。
    let manifest = out_dir.join("extrun-make.manifest");
    fs::write(&manifest, MANIFEST).expect("マニフェストを書き出せる");
    write_and_compile(
        &out_dir,
        "extrun-make.rc",
        &version_info_rc(
            "ExtRun 設定づくり",
            "extrun-make",
            "extrun-make.exe",
            Some("extrun-make.manifest"),
        ),
        "extrun-make",
    );
}

#[cfg(windows)]
fn write_and_compile(out_dir: &Path, name: &str, body: &str, bin: &str) {
    let path = out_dir.join(name);
    fs::write(&path, body).expect("リソーススクリプトを書き出せる");

    embed_resource::compile_for(&path, [bin], embed_resource::NONE)
        .manifest_required()
        .expect("リソースをコンパイルできる（Windows SDK の rc.exe が必要）");
}

/// 視覚スタイル（comctl32 v6）だけを求めるマニフェスト
///
/// **`dpiAware` は書かない。** DPI は `main()` の先頭で
/// `SetProcessDpiAwarenessContext` を呼んで宣言しており、マニフェストに
/// 書くとそちらが優先されて API の宣言が効かなくなる。宣言の場所は
/// 1 か所に保つ。
#[cfg(windows)]
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// リソーススクリプトの中身を組み立てる
///
/// `#pragma code_page(65001)` を先頭に置いて UTF-8 として読ませる。言語 ID の
/// 0x0411 は日本語、コードページの 1200 は Unicode で、ブロック名の
/// `041104B0` はその 2 つを並べたもの。
#[cfg(windows)]
fn version_info_rc(
    description: &str,
    internal_name: &str,
    original_filename: &str,
    manifest: Option<&str>,
) -> String {
    let major = env::var("CARGO_PKG_VERSION_MAJOR").expect("メジャーバージョンがある");
    let minor = env::var("CARGO_PKG_VERSION_MINOR").expect("マイナーバージョンがある");
    let patch = env::var("CARGO_PKG_VERSION_PATCH").expect("パッチバージョンがある");
    let version = env::var("CARGO_PKG_VERSION").expect("バージョンがある");

    // 1 は CREATEPROCESS_MANIFEST_RESOURCE_ID、24 は RT_MANIFEST
    let manifest_line = match manifest {
        Some(file) => format!("1 24 \"{}\"\n\n", file),
        None => String::new(),
    };

    format!(
        r#"#pragma code_page(65001)

{manifest_line}1 VERSIONINFO
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
            VALUE "FileDescription", "{description}"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "{internal_name}"
            VALUE "LegalCopyright", "Copyright (c) 2025-2026 pirukabocha"
            VALUE "OriginalFilename", "{original_filename}"
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
