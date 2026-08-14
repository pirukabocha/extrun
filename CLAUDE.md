# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 言語

コード内のコメント・ドキュメント・ユーザー向け文字列（メニュー項目名、エラーダイアログ）はすべて日本語。既存の文体に合わせること。テスト関数名も日本語。

## ビルド・実行

```powershell
cargo build --release          # target/release/extrun.exe
cargo build                    # デバッグビルド（コンソールが残る）
cargo test                     # パーサとメニュー構造のテスト
cargo clippy --all-targets
cargo fmt
```

**デバッグビルドとリリースビルドの決定的な違い**: `build.rs` はリリース時のみ `/SUBSYSTEM:WINDOWS` を指定する。つまりリリースバイナリにはコンソールが無く、`eprintln!` / `println!` の出力はどこにも表示されない。ユーザーに何かを伝える手段は `MessageBoxW`（エラーダイアログ）と `console.rs`（`--check` / `--version` / `--help`）の 2 つだけ。

同じ理由で、**リリースバイナリを PowerShell から起動すると終了を待たない**（`$LASTEXITCODE` も設定されない）。`--check` の終了コードを見るときは `Start-Process -Wait -PassThru` の `ExitCode` を使う。コマンドプロンプトは待つので `%errorlevel%` がそのまま使える。

`build.rs` は VERSIONINFO も埋め込む。リソーススクリプトは Cargo.toml のバージョンから `OUT_DIR` に生成するので、`.rc` をリポジトリに置かない（バージョンの二重管理になる）。コンパイルには Windows SDK の `rc.exe` が要る。

Windows 専用。`menu.rs` / `console.rs` は `windows-sys` を無条件に use しているため、他プラットフォームではコンパイルできない。

実行には引数としてファイル/フォルダのパスが必要:

```powershell
cargo run -- "C:\path\to\file.txt" "C:\path\to\another.png"
extrun.exe --check             # 設定ファイルの検証（引数不要。エラーがあれば終了コード 1）
extrun.exe --version           # バージョン
extrun.exe --help              # 使い方
```

## リリース

```powershell
.\packaging\build-release.ps1   # dist\extrun-<version>-win-x64.zip と .sha256
```

テスト → リリースビルド → 組み立て → zip → SHA256 を通しで行う。バージョンは `cargo metadata` 経由で `Cargo.toml` から読むので、スクリプトにハードコードしない。zip の中身は `extrun-<version>/` の下に `extrun.exe` / `readme.txt`（`packaging/readme.txt` 由来。GitHub の `README.md` とは別物）/ `extrun-config.sample.txt`（`extrun-config.txt` をリネーム）/ `extrun-config-format.md` / `extrun-recipes.md` / `LICENSE`。

- **設定ファイルを `.sample.txt` にリネームして入れるのは意図的**。`extrun-config.txt` のまま同梱すると、更新版を同じフォルダに展開したユーザーの設定が上書きで消える。
- **`LICENSE` の同梱は MIT の条件**（all copies に著作権表示を含める）なので外さない。
- 同梱するファイルを増減するときは、ビルドスクリプトと `packaging/readme.txt` の「同梱ファイル」節の両方を直す。
- バージョンを上げるときに手で直すのは `Cargo.toml` / `packaging/readme.txt` の見出し / `CHANGELOG.md` の 3 か所。readme.txt の見出しがずれているとビルドスクリプトが止まる。
- `v1.0.0` 形式のタグを push すると `.github/workflows/release.yml` が同じスクリプトを走らせ、zip と `.sha256` を**下書きの** Release に添付する。公開は GitHub 上で手動。

## アーキテクチャ

起動 → メニュー表示 → 選択されたコマンドを spawn → 即終了、という一発実行のプロセス。常駐しない。メッセージループは無く、`TrackPopupMenu` を `TPM_RETURNCMD` で同期的に呼んで戻り値からアクションを引く（`window_proc` は `DefWindowProcW` に委譲するだけの飾り）。実行中にファイルを書き出すことは一切ない。

データの流れ:

1. **`main.rs`** — 引数を絶対パスに正規化し `Target { file_type, path }` を作る。`file_type` は**フォルダなら `"folder"`、拡張子があれば小文字化した `".txt"`（先頭ドット付き）、拡張子が無ければ `"file"`**。この文字列がそのまま設定ファイルの拡張子指定と比較される。
2. **`config.rs`** — `extrun.exe` と同じディレクトリの `extrun-config.txt` を読んでパースする（カレントディレクトリではない）。書式は `extrun-config-format.md` が正典。パースは診断メッセージ（`Diag`）を集めながら進み、エラーがあれば `main.rs` が行番号付きダイアログを出して終了する。
3. **`menu.rs`** — `filter_menu_items` で対象に合う項目だけを残し、Win32 のポップアップメニューを構築、選択結果を `execute_command` で実行。
4. **`placeholder.rs`** — `PathPlaceholders` が 1 パスぶんの置換値を事前計算して保持する（HashMap を使わないのは起動速度優先の意図的な設計）。
5. **`check.rs` / `console.rs`** — `--check` 専用。検証はメニュー構築から独立しているので、将来 `--preview` を足すときもここで完結する。`console.rs` は `--version` / `--help` の出力にも使う。

### 注意の要る不変条件

- **エスケープの処理段階**: 名前とパスは**パース時に** `unescape()` で解決する。引数と作業フォルダは `^` を**残したまま**保持し、`placeholder.rs` の `replace()` が左から 1 回走査して `^X` → `X` と `$x` → 置換値 を同時に処理する。引数を先にエスケープ解決すると `^$` が `$` になったあとプレースホルダーとして拾われてしまう。
- **プレースホルダーの解釈順序**: `replace()` は `$` の直後を見て `-p` を先に判定する。この判定を崩すと `$-p` が `$-` + フルパスになる。
- **拡張子の解決はパース時に完了する**: セクション見出しの既定値 → 親項目 → 子項目、の順で継承し、`[-.jpg]`（引き算）と `[.svg]`（完全置換）を解決した最終形が `MenuItem::extensions` に入る。`menu.rs` のフィルタは自分の `extensions` だけを見ればよい。
- **拡張子の先頭ドット**: 設定側は `.txt` と書く必要がある（`file` / `folder` だけが例外）。`--check` が検出する。設定側も `to_lowercase()` してから比較する。
- **セパレーターも拡張子を持つ**: `--- [folder @画像]` のように書け、セクションの既定値も継承する。フィルタで普通の項目と同じように落ちる。
- **セパレーターの整理**: `cleanup_separators` がフィルタ後に先頭・末尾・連続する `---` を取り除く。フィルタリングを変更するときはここも合わせて確認する。期待される項目数はテストに書いてある。
- **`all_mode`（`+`）**: 1 プロセスに全パスを渡す（引数中の `$p` の位置に展開、`$p` を含まなければ末尾に追加）。既定はファイルごとに別プロセス。展開されるのは**引数がちょうど `$p` のときだけ**で、`-i$p` のように埋め込まれた場合は `targets[0]` を基準に置換されるだけ。取りこぼしに気づけないので `check.rs` の `warn_embedded_path_placeholder` が警告する。
- **起動失敗の報告**: `spawn()` の失敗はエラーダイアログにする（握りつぶさない）。個別実行では対象の数だけ同じ失敗が並ぶので、同じ理由はまとめて 1 回だけ出す。`.bat` / `.ps1` などを直接指定した場合はインタプリタ経由にする案内を足す（`CreateProcess` は起動できない）。
- **`working_dir` のプレースホルダー**は `targets[0]` を基準に解決される。未指定時は実行ファイルの親ディレクトリ。
- **`--check` のコンソール出力**: `WriteConsoleW` を先に試し、失敗したらリダイレクト先とみなして `WriteFile` で UTF-8 を書く。`GetConsoleMode` はハンドルのアクセス権に左右されるため判定に使えない（PowerShell から起動すると失敗し、CP932 で文字化けする）。

## テスト

- `config.rs` — パーサの各規則とエラー検出。
- `placeholder.rs` — エスケープとプレースホルダーの相互作用。
- `check.rs` — 実行ファイルのパスと `+` の `$p` の書き方に対する警告。
- `menu.rs` — **`extrun-config.txt` をフィクスチャとして**、20 種類の対象について構築されるメニューの項目数と並び順を突き合わせる。設定ファイルを編集すると落ちるので、そのときは期待値も合わせて更新する（期待値を一度 `0` にして走らせると、失敗メッセージに実際の数が並ぶ）。

テストコードは `cargo test` でのみコンパイルされ、リリースバイナリのサイズに影響しない。遠慮なく書いてよい。

## 設計方針

- **起動速度が最優先**。ただし体感に影響しない最適化のためにコードを複雑にしない。パース速度も起動時間の誤差の範囲なので、可読性を優先する。
- **バイナリサイズの削減**が第二の目標（`[profile.release]` は `opt-level = "z"` / `lto = "fat"` / `codegen-units = 1` / `strip` / `panic = "abort"`）。
- **実行時の依存クレートは `windows-sys` のみ。増やさない。** ビルド時の依存は VERSIONINFO の埋め込みに使う `embed-resource` だけで、バイナリには入らない。こちらも増やさない。
- 旧 YAML 形式との互換は不要（`extrun.yaml` の読み込みコードは残さない）。テーマ機能は削除済み（`TrackPopupMenu` の配色を変えるには `MF_OWNERDRAW` の自前実装が必要なため）。

## 関連ドキュメント

- **`extrun-config-format.md`** — 設定ファイル形式の完全仕様。パーサに手を入れるときの正典。
- **`extrun-config.txt`** — 公開用のサンプル設定（兼テストフィクスチャ）。書式のほぼすべてを一通り使ってある。**Windows 標準のコマンドだけで動くことが不変条件**で、`--check` は `問題は見つかりませんでした` にならなければいけない。項目を足すときは次に注意する。
  - パスは素の Windows 10/11 に必ず存在するものだけ（`mspaint.exe` と WordPad は Windows 11 には無い）。実機で `Test-Path` を確認してから書く。
  - PowerShell に渡すコマンドは**全体を `"..."` で囲んで 1 引数にする**。分割すると空白入りパスで壊れる。中では `'...'` を使う。
  - `@別名` の直後に `'` を置かない（引用符は名前の終端にならないので `tar'` を探しに行く）。`@sys\tar.exe` のように `\` を挟む。
  - ユーザーのファイルを書き換える項目は避け、新しいファイルを作る形にする。
  - パーサを通した最終的な引数は、`filter_menu_items` → `PathPlaceholders::replace_args` を呼ぶ一時テストでダンプして、`ProcessStartInfo.ArgumentList`（Rust の `Command` と同じクォート規則）で実行して確かめられる。
- **`extrun-recipes.md`** — 外部アプリ（ffmpeg / ImageMagick / IrfanView / 7-Zip / VS Code / VLC / Pandoc / 画像最適化系）を使った設定例集。`extrun-config.txt` を「Windows 標準コマンドだけで動く」に絞った代償を引き受ける文書で、**サンプル設定の「必ず動く」不変条件は適用されない**（各自のインストール先に依存するため、想定バージョンを明記してパスは読み替え前提）。各レシピに「使用: `$-p` / `[-.mp4]` / `+`」の注記を付けて書式の逆引き教材を兼ねさせているので、レシピを足すときもこの形を守る。付録 A に書式 → レシピの逆引き表、付録 C に AutoHotkey の呼び出し例がある。なお「元のファイルを書き換えず新しいファイルを作る」という方針は、サンプル設定と同じくこちらでも守ること。
- **`packaging/readme.txt`** — 配布 zip に入れる説明書。`README.md` の要約ではなく、zip を展開した人向けの独立した文書。見出しのバージョンは `Cargo.toml` と揃える（`build-release.ps1` が検査する）。
- **`CHANGELOG.md`** — 変更履歴。Keep a Changelog 形式。リリースのたびに追記する。
- **`.github/workflows/`** — `ci.yml`（`fmt` / `clippy -D warnings` / `test` / サンプル設定の `--check`）と `release.yml`（タグから zip を作って下書き Release に添付）。

同じ話が README.md・`extrun-config-format.md`・`packaging/readme.txt` の 3 か所に出てくることがある（右クリック登録、`--check`、トラブルシューティング）。片方だけ直すとずれるので、書き換えるときは横断で確認する。
