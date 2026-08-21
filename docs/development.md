# 開発

ExtRun をソースからビルドする人と、手を入れる人のための覚え書き。使うだけなら [README](../README.md) と [設定ファイル仕様](extrun-config-format.md)で足ります。

## ビルド環境

- Rust 2024 Edition（`rust-version = 1.85.0`）
- Windows 11 / 10
- Windows SDK の `rc.exe`（VERSIONINFO の埋め込みに使用。Visual Studio Build Tools を入れていれば揃っています）

```powershell
cargo build --release      # target/release/extrun.exe
cargo build                # デバッグビルド（コンソールが残る）
cargo test
cargo clippy --all-targets
cargo fmt --check
```

同じ内容を GitHub Actions（[ci.yml](../.github/workflows/ci.yml)）が `windows-latest` で実行します。テストが同梱サンプルと `tests/fixtures/full-config.txt` の両方を読むので、設定ファイルの書式エラーも CI で検出されます。**画面が要るテスト（`#[ignore]` 付き）も CI で毎回走ります**（`cargo test -- --ignored --test-threads=1`）。

> [!IMPORTANT]
> **リリースビルドにはコンソールがありません。** `build.rs` がリリース時だけ `/SUBSYSTEM:WINDOWS` を指定するため、`println!` / `eprintln!` の出力はどこにも出ません。同じ理由で、リリースバイナリを PowerShell から起動しても**終了を待たず**、`$LASTEXITCODE` も設定されません。`--check` の終了コードを見るときは `Start-Process -Wait -PassThru` の `ExitCode` を使ってください（コマンドプロンプトは待つので `%errorlevel%` がそのまま使えます）。

Windows 専用です。`menu.rs` / `console.rs` が `windows-sys` を無条件に `use` しているため、他のプラットフォームではコンパイルできません。

## 依存クレート

- `windows-sys` — Windows API（メニュー・ダイアログ・アイコン・クリップボード・コンソール出力）

**実行時の依存はこれだけです。** 設定ファイルのパーサも、日時の書式も、入力ダイアログも自前実装です。

ビルド時のみ `embed-resource` を使い、exe に VERSIONINFO（プロパティの「詳細」タブに出るバージョンや著作権表記）を埋め込みます。バイナリには含まれず、サイズにも影響しません。リソーススクリプトは `Cargo.toml` のバージョンから `OUT_DIR` に生成するので、`.rc` はリポジトリに置いていません（バージョンの二重管理になるため）。

## 設計方針

- **起動速度が最優先。** ただし体感に影響しない最適化のためにコードを複雑にしません
- **バイナリサイズの削減が第二の目標。** `[profile.release]` は `opt-level = "z"` / `lto = "fat"` / `codegen-units = 1` / `strip` / `panic = "abort"`
- 起動時のファイル I/O は設定ファイルの読み込み 1 回のみ。**実行時の書き込みはゼロ**（レジストリも含む）
- 常駐しません。メニューを出して、選ばれたコマンドを起動して、終了します

## プロジェクト構成

```text
extrun/
├── src/
│   ├── main.rs         # エントリポイント、引数処理、DPI 宣言
│   ├── lib.rs          # 中身の入口（モジュール宣言、Target、エラーダイアログ）
│   ├── bin/extrun-make/    # 設定づくり（2 つ目の実行ファイル）
│   │   ├── main.rs         #   ダイアログ手続き、フォームの読み書き
│   │   ├── form.rs         #   設定ファイルの数行を組み立てる（テストの本体）
│   │   ├── layout.rs       #   画面の寸法（DLGTEMPLATE）
│   │   ├── live.rs         #   ライブプレビュー（起動されるコマンドライン）
│   │   ├── iconpick.rs     #   :icon の番号を一覧から選ぶ画面
│   │   ├── existing.rs     #   今ある設定ファイルを読む（別名・プレビューの前置き）
│   │   ├── edits.rs        #   入力欄に Ctrl+A を効かせる
│   │   ├── presets.rs      #   対象の種類のひな型
│   │   └── clip.rs         #   クリップボードとファイル選択
│   ├── config.rs       # 設定ファイルのパース
│   ├── text.rs         # エスケープ・区切り・%NAME% の展開（パーサと実行時で共用）
│   ├── menu.rs         # Win32 メニューの構築・表示・実行
│   ├── filter.rs       # 対象に合う項目だけを残す
│   ├── invoke.rs       # 起動 1 回ぶんの組み立て（実行と --preview で共用）
│   ├── launch.rs       # プロセスの起動、:delay / :wait / :admin
│   ├── confirm.rs      # 入力欄と実行前の確認
│   ├── placeholder.rs  # プレースホルダー置換と RunContext
│   ├── datetime.rs     # $t{...} の書式解釈
│   ├── prompt.rs       # $?{...} の入力ダイアログ
│   ├── progress.rs     # :delay / :wait の進行状況ダイアログと中止後の要約
│   ├── dialog.rs       # DLGTEMPLATE の組み立て（prompt.rs / progress.rs 共用）
│   ├── icon.rs         # :icon のアイコン取り出し
│   ├── check.rs        # --check の検証と整形
│   ├── preview.rs      # --preview の整形
│   └── console.rs      # コンソールへの出力
├── docs/
│   ├── extrun-config-format.md # 設定ファイルの仕様
│   ├── extrun-recipes.md       # 実用的な設定例集（3 章は追加インストール不要）
│   ├── development.md          # このファイル
│   └── images/                 # README で使う画像
├── packaging/
│   ├── build-release.ps1       # 配布用 zip の作成
│   └── readme.txt              # 配布物に同梱する説明書
├── tests/fixtures/
│   └── full-config.txt         # 書式を一通り使ったテスト用フィクスチャ（配布しない）
├── .github/workflows/
│   ├── ci.yml                  # fmt / clippy / test / 実機テスト / 設定ファイルの --check
│   └── release.yml             # タグから zip を作って下書き Release に添付
├── build.rs                    # サブシステムと VERSIONINFO
├── extrun-config.txt           # 同梱するサンプル設定（最小限）
├── CHANGELOG.md
└── README.md
```

設定ファイルは 2 つあります。**`extrun-config.txt` は配布物に入る同梱サンプル**で、初めて開く人が読み通せる最小限の内容に保ちます。**`tests/fixtures/full-config.txt` は書式を一通り使ったテストフィクスチャ**で、配布物には入りません。`filter.rs` のテストは両方をパースし、後者についてはメニュー構造（項目数と並び順）まで突き合わせるので、編集したら期待値も合わせて更新してください。

書式の実例を足したくなったら、フィクスチャではなく [extrun-recipes.md](extrun-recipes.md) の方に書きます（利用者が読める場所にあるのはそちらです）。

## リリース用パッケージの作成

```powershell
.\packaging\build-release.ps1
```

テスト → リリースビルド → 配布物の組み立て → zip → SHA256 を通しで実行し、`dist\` に出力します。バージョンは `cargo metadata` 経由で `Cargo.toml` から読むので、スクリプトにはハードコードしていません。

```text
dist/
├── extrun-<version>-win-x64.zip
└── extrun-<version>-win-x64.zip.sha256

zip の中身:
extrun-<version>/
├── extrun.exe
├── readme.txt                 # packaging/readme.txt。README.md とは別物
├── extrun-config.sample.txt   # extrun-config.txt をリネームしたもの（最小限のサンプル）
├── extrun-config-format.md    # docs/ から。zip の中ではフラットに並べる
├── extrun-recipes.md          # 同上
├── CHANGELOG.md
└── LICENSE
```

- 設定ファイルを `.sample.txt` にリネームして入れるのは意図的です。`extrun-config.txt` のまま同梱すると、更新版を同じフォルダに展開した人の設定が上書きで消えます
- `LICENSE` の同梱は MIT の条件（all copies に著作権表示を含める）なので外しません
- 同梱するファイルを増減するときは、ビルドスクリプトと `packaging/readme.txt` の「同梱ファイル」節の両方を直します

バージョンを上げるときに手で直すのは **`Cargo.toml` / `packaging/readme.txt` の見出し / `CHANGELOG.md` の 3 か所**です。readme.txt の見出しがずれているとビルドスクリプトが止まります。`Cargo.lock` は `cargo build` が追随するので手作業は不要です。

`v1.2.3` のようなタグを push すると、[release.yml](../.github/workflows/release.yml) が同じスクリプトを実行して、zip と `.sha256` を**下書きの** Release に添付します。公開は GitHub 上で手動です。

## 使用している主な Windows API

| API | 用途 |
|---|---|
| `CreatePopupMenu` / `AppendMenuW` / `TrackPopupMenu` | メニューの構築と表示（`TPM_RETURNCMD` で同期的に選択結果を得る） |
| `SetMenuItemInfoW`（`MIIM_BITMAP`） | 項目へのアイコン付与。`MF_OWNERDRAW` は使わない |
| `SHDefExtractIconW` / `DrawIconEx` | `:icon` のアイコン取り出しと 32bpp DIB への描画 |
| `DialogBoxIndirectParamW` | `$?{...}` の入力欄と `:delay` / `:wait` の進行状況（`DLGTEMPLATE` をメモリ上に手組み） |
| `SetTimer` / `KillTimer` | `:delay` の待ち時間と `:wait` の様子見（ダイアログのモーダルループが汲む。`thread::sleep` では中止できない） |
| `WaitForSingleObject` / `SEE_MASK_NOCLOSEPROCESS` | `:wait` で `:admin` のプロセスの終了を見る（通常の起動は `std` の `Child::try_wait`） |
| `SetClipboardData`（`CF_UNICODETEXT`） | 中止したときに残りのパスを渡す（ファイルは書き出さない） |
| `MessageBoxW` | `:confirm` の確認とエラーダイアログ |
| `GetLocalTime` | `$t{...}` の日時 |
| `SetProcessDpiAwarenessContext` | Per-Monitor DPI Awareness V2 の宣言 |
| `SendInput` | `select-first` の初期選択（メニューのモーダルループには `PostMessageW` が届かない） |
| `AttachConsole` / `WriteConsoleW` | `--check` / `--preview` / `--version` / `--help` の出力 |

## テスト

パーサ、日時の書式、プレースホルダーとエスケープの相互作用、入力欄の書式、`--preview` の整形、アイコンのビットマップ、`--check` の各警告、コマンドライン引数の切り出し、そして `tests/fixtures/full-config.txt` から構築されるメニュー構造を検査します。

```powershell
cargo test              # 通常のテスト（ライブラリ / バイナリ / doctest）
cargo test --lib        # ライブラリだけ（大半のテストはここ）
cargo test --bin extrun-make  # 設定づくりの組み立てだけ
cargo test -- --ignored --test-threads=1  # 実機でしか確かめられないもの（ダイアログの表示と :wait の逐次起動）
```

テストコードは `cargo test` でのみコンパイルされ、リリースバイナリのサイズに影響しません。

内部の作りについての詳しい注意点（不変条件、なぜその実装なのか）は、リポジトリ直下の `CLAUDE.md` にまとめてあります。
