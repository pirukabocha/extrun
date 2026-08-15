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

テスト → リリースビルド → 組み立て → zip → SHA256 を通しで行う。バージョンは `cargo metadata` 経由で `Cargo.toml` から読むので、スクリプトにハードコードしない。zip の中身は `extrun-<version>/` の下に `extrun.exe` / `readme.txt`（`packaging/readme.txt` 由来。GitHub の `README.md` とは別物）/ `extrun-config.sample.txt`（`extrun-config.txt` をリネーム）/ `extrun-config-format.md` / `extrun-recipes.md` / `CHANGELOG.md` / `registry/extrun-add.reg` と `registry/extrun-remove.reg`（`packaging/registry/` 由来）/ `LICENSE`。

- **`.reg` の文字コードは `.gitattributes` の `working-tree-encoding` が担う**。regedit は BOM で文字コードを判定し、BOM が無ければ ANSI と見なすので、UTF-8 のままだとメニュー名の日本語が文字化けする。`*.reg` に `working-tree-encoding=UTF-16LE-BOM` を指定してあるので、**リポジトリには UTF-8 + LF で格納され（差分も blame も GitHub の表示も読める）、作業ツリー・clone・`git archive` には UTF-16 LE + BOM + CRLF で出てくる**。clone した人がリポジトリ内の `.reg` をそのままコピーして使っても壊れない。作業ツリー側を UTF-8 で保存し直すと `git add` が「failed to encode」で止まるので、取り違えたまま進むことはない。`build-release.ps1` の `Copy-AsRegFile` は保険として残してある（Git を経由せず配布物を組む場合に効く）。

- **設定ファイルを `.sample.txt` にリネームして入れるのは意図的**。`extrun-config.txt` のまま同梱すると、更新版を同じフォルダに展開したユーザーの設定が上書きで消える。
- **`LICENSE` の同梱は MIT の条件**（all copies に著作権表示を含める）なので外さない。
- 同梱するファイルを増減するときは、ビルドスクリプトと `packaging/readme.txt` の「同梱ファイル」節の両方を直す。
- バージョンを上げるときに手で直すのは `Cargo.toml` / `packaging/readme.txt` の見出し / `CHANGELOG.md` の 3 か所。readme.txt の見出しがずれているとビルドスクリプトが止まる。`Cargo.lock` は `cargo build` が追随するので手作業は不要。上げたら `.\packaging\build-release.ps1` を通して、zip 名・`--version`・exe の VERSIONINFO まで揃っているか確かめる。
- **ドキュメントの例示にバージョン番号を書かない。** 版が上がるたびに腐り、更新箇所が増えていく。`extrun-<version>-win-x64.zip` のような書き方にするか、明らかに架空の `v1.2.3` を使う。
- **開発中はバージョンを上げない。** 変更のたびに `CHANGELOG.md` の `[Unreleased]` へ追記していき、リリースを決めた時点で上の 3 か所を 1 回だけ揃える。`Cargo.toml` の値は VERSIONINFO として exe に埋め込まれ、zip 名と readme.txt の見出し照合にも使われる「配布物の身元」なので、開発中に上げると同じ番号を名乗る未公開バイナリが増えて不具合報告と対応づかなくなる。minor か major かも変更が出揃うまで決まらない。
- `v1.2.3` 形式のタグを push すると `.github/workflows/release.yml` が同じスクリプトを走らせ、zip と `.sha256` を**下書きの** Release に添付する。公開は GitHub 上で手動。

## アーキテクチャ

起動 → メニュー表示 → 選択されたコマンドを spawn → 即終了、という一発実行のプロセス。常駐しない。自前のメッセージループは無く、`TrackPopupMenu` を `TPM_RETURNCMD` で同期的に呼んで戻り値からアクションを引く（`window_proc` はほぼ `DefWindowProcW` への委譲で、`select-first` のタイマーだけを受ける）。実行中にファイルを書き出すことは一切ない。

データの流れ:

1. **`main.rs`** — 引数を絶対パスに正規化し `Target { file_type, path }` を作る。`file_type` は**フォルダなら `"folder"`、拡張子があれば小文字化した `".txt"`（先頭ドット付き）、拡張子が無ければ `"file"`**。この文字列がそのまま設定ファイルの拡張子指定と比較される。
2. **`config.rs`** — `extrun.exe` と同じディレクトリの `extrun-config.txt` を読んでパースする（カレントディレクトリではない）。書式は `extrun-config-format.md` が正典。パースは診断メッセージ（`Diag`）を集めながら進み、エラーがあれば `main.rs` が行番号付きダイアログを出して終了する。
3. **`menu.rs`** — `filter_menu_items` で対象に合う項目だけを残し、Win32 のポップアップメニューを構築、選択結果を `execute_command` で実行。
4. **`placeholder.rs`** — `PathPlaceholders` が 1 パスぶんの置換値を事前計算して保持する（HashMap を使わないのは起動速度優先の意図的な設計）。`RunContext` は**対象のパスから導けない実行時の値**（現在は日時のみ）を持ち、対象をまたいで共有される。
5. **`datetime.rs`** — 日時プレースホルダー `$t{...}` の書式解釈と検証。`LocalTime` は `GetLocalTime` を包むだけで、書式の対応表は `LocalTime::field` の 1 か所。
6. **`prompt.rs`** — `$?{...}` の入力ダイアログと書式の解釈。Win32 に既製の入力ボックスが無いので、`DLGTEMPLATE` をメモリ上に組み立てて `DialogBoxIndirectParamW` に渡す。
7. **`check.rs` / `preview.rs` / `console.rs`** — `--check` と `--preview` 専用。どちらもメニュー構築から独立している。`console.rs` は `--version` / `--help` の出力にも使う。

### 注意の要る不変条件

- **表示と実行がずれない**: 起動されるプロセスを組み立てるのは `menu::resolve_invocations` の 1 か所だけで、実行経路（`execute_command`）と `--preview` の両方がここを通る。コマンドラインの組み立てを他所に書くと、プレビューが嘘をつくようになる。
- **実行時に決まる値は `RunContext` に集める**: 日時は `execute_command` と `preview::run` の入口で 1 回だけ確定させ、全対象で共有する。対象ごとに取り直すと、複数選択の個別実行で `$t{ss}` がずれてファイル名が揃わなくなる。将来 `$?{...}`（入力プロンプト）のような「パスから導けない値」を足すときもここに載せる。
- **`$t` は `$t{` のときだけ日時**: 単独の `$t` を素通しするのは、既存の設定ファイルの意味を変えないため。書式の対応表は `datetime.rs` の `LocalTime::field` だけに置き、`validate_spec` もその関数を呼んで判定する（表が 2 か所に分かれると片方だけ直す事故になる）。中括弧の中に書式以外の英字を書けないのは、引用記法を増やさずに書き間違いを `--check` で全部拾うための割り切り。**書式の誤りは警告ではなくエラー**（黙って誤った文字列がファイル名に入るのを防ぐ）。
- **`$?{...}` は答えを先に集めてから置換する**: `replace()` は対象の数だけ呼ばれるので、その場で聞くと入力欄が何度も出る。`menu::ask_prompts` が起動より前に全部聞いて `RunContext` に入れ、`replace()` は引くだけにする。順番は**入力欄 → `:confirm` → 起動**（確認のメッセージに入力した値を入れられる）。**ひとつでもキャンセルされたら実行しない**（半端な入力のまま起動すると意図しない引数でコマンドが走る）。`$?{...}` の終端を探すときは `prompt::find_close` で中括弧の深さを数える（中に `$t{...}` を書けるようにするため）。`placeholder.rs` の `replace()` も同じ関数を使う。
- **`RunContext` の入力欄の見出しは書かれた文字列全体**（`$?int{幅}`）: 中括弧の中身だけにすると `$?{幅}` と `$?int{幅}` が同じ答えを共有する。書き方を解釈するのは `prompt::parse_at` の 1 か所で、`placeholder.rs` と `config::split_args` はそれを呼ぶ。
- **入力値の決まりは閉じた集合**（`int` / `num` / `name`）: 正規表現を入れないのは、依存クレートが増えて「実行時の依存は `windows-sys` のみ」に反するため。文字数指定を入れないのは、防げる事故が具体的に無いから。**`name` の 255 文字だけは固定の上限として組み込む**（NTFS の名前 1 要素の上限で、ユーザーが決める値ではない）。パス全体の `MAX_PATH` は入力値がどこに入るか分からないので見ない。**決まりを満たさない入力は打ち切らず聞き直す**（打ち切ると「キャンセルと同じで何も起きない」になり、検証がかえって邪魔になる）。対応表は `Rule::check` の 1 か所で、`--check` の既定値の検証も同じ関数を通る。
- **`split_args` は `$t{` / `$?{` の中では区切らない**: `$?{$n の新しい名前}` のように説明に空白が入るため。**素の `{` は数えない**（数えると PowerShell のスクリプトブロックが引数をまたいで繋がる）。
- **ダイアログテンプレートは手で組む**: Win32 に入力欄つきダイアログの API が無いので、`prompt.rs` が `DLGTEMPLATE` をメモリ上に組み立てる。**各項目は DWORD 境界から始める**必要があり、先頭も DWORD 境界に置く（`Vec<u16>` では足りないので `Vec<u32>` に載せ替える）。組み立てを誤ると `DialogBoxIndirectParamW` が `-1` を返すだけなので、実機で通す `#[ignore]` 付きのテストを置いてある（`cargo test -- --ignored`）。
- **エスケープの処理段階**: 名前は**パース時に** `unescape_name()`、パスは `unescape()` で解決する。引数・作業フォルダ・`:confirm` のメッセージは `^` を**残したまま**保持し、`placeholder.rs` の `replace()` が左から 1 回走査して `^X` → `X` と `$x` → 置換値 を同時に処理する。引数を先にエスケープ解決すると `^$` が `$` になったあとプレースホルダーとして拾われてしまう。`SPECIALS` は `config.rs` の 1 か所だけに置き、`placeholder.rs` はそれを use する（かつて 2 か所に重複定義されていて、片方だけ直すと `^X` が実行時に素通りする事故になった）。
- **アクセスキーは名前欄だけの記法**: `MenuItem::name` は**表示用の文字列**で、アクセスキーの `&` は含まない（`^&` は素の `&` に解決済み）。位置は `MenuItem::accesskey`（`name` のバイト位置）が持ち、Win32 に渡すラベルは `menu.rs` の `to_label_wide()` が組み立て直す。**このとき表示名の中の `&` を `&&` に二重化する**（Win32 のメニューは `&` をアクセスキーの目印として食べるため）。`unescape_name()` が 1 パスでエスケープ解決とキーの読み取りを兼ねているのは、先に `^&` を `&` にするとそれがキーの目印として拾われるから。引数欄の `&` は PowerShell の呼び出し演算子として素通しする。
- **アクセスキーのスコープ**: Win32 のニーモニックはポップアップごとにスコープされるので、`check.rs` の `warn_duplicate_accesskeys` が比べるのは兄弟だけ。親と子で同じ文字を使ってよい。ただし**セクションが違ってもルートの項目は同じ階層**で、複数選択時のメニューは和集合になるため、ルートのキーは設定ファイル全体で一意にする必要がある（サンプル設定の不変条件）。
- **プレースホルダーの解釈順序**: `replace()` は `$` の直後を見て `-p` を先に判定する。この判定を崩すと `$-p` が `$-` + フルパスになる。
- **拡張子の解決はパース時に完了する**: セクション見出しの既定値 → 親項目 → 子項目、の順で継承し、`[-.jpg]`（引き算）と `[.svg]`（完全置換）を解決した最終形が `MenuItem::extensions` に入る。`menu.rs` のフィルタは自分の `extensions` だけを見ればよい。
- **拡張子の先頭ドット**: 設定側は `.txt` と書く必要がある（`file` / `folder` だけが例外）。`--check` が検出する。設定側も `to_lowercase()` してから比較する。
- **セパレーターも拡張子を持つ**: `--- [folder @画像]` のように書け、セクションの既定値も継承する。フィルタで普通の項目と同じように落ちる。
- **セパレーターの整理**: `cleanup_separators` がフィルタ後に先頭・末尾・連続する `---` を取り除く。フィルタリングを変更するときはここも合わせて確認する。期待される項目数はテストに書いてある。
- **`all_mode`（`+`）**: 1 プロセスに全パスを渡す（引数中の `$p` の位置に展開、`$p` を含まなければ末尾に追加）。既定はファイルごとに別プロセス。展開されるのは**引数がちょうど `$p` のときだけ**で、`-i$p` のように埋め込まれた場合は `targets[0]` を基準に置換されるだけ。取りこぼしに気づけないので `check.rs` の `warn_embedded_path_placeholder` が警告する。
- **起動失敗の報告**: `spawn()` の失敗はエラーダイアログにする（握りつぶさない）。個別実行では対象の数だけ同じ失敗が並ぶので、同じ理由はまとめて 1 回だけ出す。`.bat` / `.ps1` などを直接指定した場合はインタプリタ経由にする案内を足す（`CreateProcess` は起動できない）。
- **`working_dir` のプレースホルダー**は `targets[0]` を基準に解決される。未指定時は実行ファイルの親ディレクトリ。
- **`[extrun]` は予約セクション名**: 拡張子は `.` で始まる必要がある（`file` / `folder` だけが例外）ので、この名前は元々エラーになる書き方だった。`build_menu` が見出しを見てモードを切り替え、以降の行を項目ではなく `名前 = 値` として読む。次の拡張子セクションで項目に戻る。**拡張子セクションと違い位置に依存しない**（どこに書いても全体に効く）。
- **表示位置の値の解釈は `config::parse_menu_position` の 1 か所だけ**: 設定ファイルの `menu-position` と `--at` の両方から呼ぶ。片方にだけ書き方を足すとずれる。
- **メニューにキー入力を差し込むには `SendInput` が要る**: `PostMessageW` でオーナーウィンドウに `WM_KEYDOWN` を投げても**メニューには届かない**（`window_proc` に配送されて `DefWindowProcW` に捨てられる）。メニューは自分のモーダルループで本物のキー入力を読むため。`select-first` は `TrackPopupMenu` の前に `SetTimer` を仕掛け、モーダルループが手を空けたところで届く `WM_TIMER` の中から `SendInput` する。タイマーは `select-first` が有効なときしか仕掛けない。
- **`GetForegroundWindow` は `SetForegroundWindow` より前に取る**: `create_and_show_menu` の先頭で捕まえている。自分が前面に出たあとに取ると自分自身になり、`--at window` が壊れる。**最小化されたウィンドウの矩形は `-32000` 付近を返す**ので `IsIconic` で弾かないと画面外にメニューが出る。基準にできないときは画面中央 → カーソル位置の順に落とす。
- **DPI 対応はウィンドウを作る前に済ませる**: `main()` の先頭の `enable_dpi_awareness()`（Per-Monitor V2）が唯一の宣言箇所。マニフェストではなく API で宣言しているので、**この呼び出しより前にウィンドウや DPI 依存の API を触ってはいけない**。宣言しないと座標が仮想化され、`GetCursorPos` などが物理ピクセルを返さなくなるため、位置計算のコードはこれが効いている前提で書く。V2 でなければならないのは、メニューの自動スケーリングが V2 で追加された機能だから。
- **`--check` のコンソール出力**: `WriteConsoleW` を先に試し、失敗したらリダイレクト先とみなして `WriteFile` で UTF-8 を書く。`GetConsoleMode` はハンドルのアクセス権に左右されるため判定に使えない（PowerShell から起動すると失敗し、CP932 で文字化けする）。

## テスト

- `config.rs` — パーサの各規則とエラー検出。
- `datetime.rs` — `$t{...}` の書式ごとの出力と、`--check` が出すエラーの検出。
- `placeholder.rs` — エスケープとプレースホルダーの相互作用（日時の書式そのものは `datetime.rs` の担当で、ここで見るのはパスやエスケープと同居させたときの振る舞い）。
- `prompt.rs` — `$?{...}` の書式（説明と既定値の分け方、中括弧の対応、`--check` のエラー）。ダイアログ本体は `#[ignore]` 付きのテストで実機のときだけ確かめる。
- `preview.rs` — `--preview` の整形。時刻を固定した `RunContext` を渡して突き合わせる。
- `check.rs` — 実行ファイルのパスと `+` の `$p` の書き方、アクセスキーの重複、届かない `:confirm` に対する警告。
- `main.rs` — `take_options` によるコマンドライン引数の切り出し（`--at` / `--select-first` / `--no-select-first`）。`Overrides` は `Option` で「指定なし」と「明示的に no」を区別する。
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
  - アクセスキーを足すときは**ルート階層だけセクションをまたいで一意**にする（サブメニューの中は独立）。`--check` が重複を検出する。`(&O)` を末尾に足すと表示名が変わるので `menu.rs` の項目名テストも直す（`&PNG` のように名前の中の文字に付ける場合は変わらない）。
  - パーサを通した最終的な引数は、`filter_menu_items` → `PathPlaceholders::replace_args` を呼ぶ一時テストでダンプして、`ProcessStartInfo.ArgumentList`（Rust の `Command` と同じクォート規則）で実行して確かめられる。
- **`extrun-recipes.md`** — 外部アプリ（ffmpeg / ImageMagick / IrfanView / 7-Zip / VS Code / VLC / Pandoc / 画像最適化系）を使った設定例集。`extrun-config.txt` を「Windows 標準コマンドだけで動く」に絞った代償を引き受ける文書で、**サンプル設定の「必ず動く」不変条件は適用されない**（各自のインストール先に依存するため、想定バージョンを明記してパスは読み替え前提）。各レシピに「使用: `$-p` / `[-.mp4]` / `+`」の注記を付けて書式の逆引き教材を兼ねさせているので、レシピを足すときもこの形を守る。付録 A に書式 → レシピの逆引き表、付録 C に AutoHotkey の呼び出し例がある。なお「元のファイルを書き換えず新しいファイルを作る」という方針は、サンプル設定と同じくこちらでも守ること。
- **`packaging/readme.txt`** — 配布 zip に入れる説明書。`README.md` の要約ではなく、zip を展開した人向けの独立した文書。見出しのバージョンは `Cargo.toml` と揃える（`build-release.ps1` が検査する）。
- **`CHANGELOG.md`** — 変更履歴。Keep a Changelog 形式。リリースのたびに追記する。**配布 zip にも同梱するので、利用者から見て何が変わるかを書く**（リファクタリング・テストの追加・CI・ドキュメントの内部整理は載せない。それは git log の担当）。設定ファイルの見直しが要る変更は必ず「変更」の欄に入れる。利用者はそこだけ読んで更新の可否を判断する。
- **`.github/workflows/`** — `ci.yml`（`fmt` / `clippy -D warnings` / `test` / サンプル設定の `--check`）と `release.yml`（タグから zip を作って下書き Release に添付）。

同じ話が README.md・`extrun-config-format.md`・`packaging/readme.txt` の 3 か所に出てくることがある（右クリック登録、`--check`、トラブルシューティング）。片方だけ直すとずれるので、書き換えるときは横断で確認する。
