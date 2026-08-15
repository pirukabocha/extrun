# ExtRun

**ExtRun** は、拡張子に関連付けられたコンテキストメニューから、ファイルやフォルダを任意のアプリで開く Windows 用ランチャーです。

> **In English** — ExtRun is a tiny Windows launcher written in Rust. Pass it file or folder paths and it pops up a context menu at the cursor, showing only the commands that apply to those file types, then spawns the one you pick and exits. Menus are defined in a single plain-text config file (one line per entry) next to the executable. No installer, no background process, no registry writes. Documentation is in Japanese; the config file format is summarized under [設定ファイル](#設定ファイル-extrun-configtxt) and specified in full in [extrun-config-format.md](extrun-config-format.md).

![ExtRun のメニュー](docs/images/menu.png)

## 特徴

- **⚡ 超高速起動**: Rust 製ネイティブアプリケーション
- **🎯 拡張子ベースフィルタリング**: ファイルの種類に応じて適切なアプリだけを表示
- **📁 複数ファイル対応**: 一度に複数のファイル/フォルダを処理可能
- **🔧 柔軟な設定**: 1行1項目のシンプルな設定ファイル
- **🖱️ マウスとキーボードの両方に配慮**: ユーザーの入力環境を問わず利用しやすい操作感
- **💾 省メモリ**: 実行時の依存クレートは `windows-sys` のみ

## インストール

### バイナリを使用する場合

1. リリースページから `extrun-<version>-win-x64.zip` をダウンロード
2. 任意のフォルダに展開
3. `extrun-config.sample.txt` をコピーして `extrun-config.txt` にリネーム
4. 任意で右クリックメニューに登録（下記）

同梱のサンプル設定は Windows 標準のコマンドだけで動くので、追加のインストールなしでそのまま試せます。

設定ファイルが `extrun-config.sample.txt` という名前で入っているのは、更新版を同じフォルダに展開したときに、書き換えた `extrun-config.txt` を上書きで消さないためです。

ffmpeg・7-Zip・ImageMagick・VS Code といった手持ちのアプリを登録する段階になったら、[extrun-recipes.md](extrun-recipes.md)（レシピ集）にそのまま貼って使える設定例をまとめてあります。

### ソースからビルドする場合

```powershell
# リリースビルド（推奨）
cargo build --release

# 実行ファイルの場所
# target/release/extrun.exe
```

## 使用方法

### 基本的な使い方

```powershell
# 単一ファイルを開く
extrun.exe document.txt

# 複数ファイルを同時に処理
extrun.exe image1.jpg image2.jpg image3.jpg

# フォルダを開く
extrun.exe C:\Projects\MyProject

# 設定ファイルを検証する
extrun.exe --check

# 実際に起動されるコマンドラインを、起動せずに表示する
extrun.exe --preview image.jpg

# バージョン / ヘルプ
extrun.exe --version
extrun.exe --help
```

コマンドを実行すると、カーソル位置にコンテキストメニューが表示されます。

### Windows エクスプローラとの統合

> [!NOTE]
> AutoHotkey ユーザー向けに、便利なスクリプトを [extrun-recipes.md](extrun-recipes.md#付録-c-autohotkey-から呼び出す) に付録として記載していますので、そちらも参考にして下さい。

右クリックメニューに追加することで、エクスプローラから直接使用できます。

一番簡単なのは「送る」メニューに登録することです：

1. エクスプローラのアドレスバーに `shell:sendto` と入力
2. 開いたフォルダに `extrun.exe` のショートカットを置く

これで、右クリック →「送る」→ ExtRun で選択中のファイルを実行できます。

レジストリを編集し、右クリックメニューに項目として追加する方法もあります。**配布 zip には、この内容を書き込み済みの `registry\extrun-add.reg` / `registry\extrun-remove.reg` が入っています**ので、そちらを使えばパスを書き換えるだけで済みます。

自分で作る場合は以下の内容を `.reg` ファイルとして保存し、`extrun.exe` のパスを実際の置き場所に書き換えてから実行してください。

> [!IMPORTANT]
> **`.reg` は UTF-16 LE（BOM あり）で保存してください。** regedit は BOM で文字コードを判定し、BOM が無いファイルは ANSI として読みます。メモ帳の既定は UTF-8 なので、そのまま保存するとメニュー名の日本語が文字化けするか、インポートに失敗します。メモ帳なら「名前を付けて保存」で文字コードに `UTF-16 LE` を選んでください。

```registry
Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\Classes\*\shell\ExtRun]
@="ExtRun で開く(&E)"

[HKEY_CURRENT_USER\Software\Classes\*\shell\ExtRun\command]
@="\"C:\\Tools\\extrun\\extrun.exe\" \"%1\""

[HKEY_CURRENT_USER\Software\Classes\Directory\shell\ExtRun]
@="ExtRun で開く(&E)"

[HKEY_CURRENT_USER\Software\Classes\Directory\shell\ExtRun\command]
@="\"C:\\Tools\\extrun\\extrun.exe\" \"%1\""
```

`*` がファイル、`Directory` がフォルダ向けの登録です。`HKEY_CURRENT_USER\Software\Classes` に書くので**管理者権限は不要**で、現在のユーザーにだけ適用されます（`HKEY_CLASSES_ROOT` に直接書くと昇格が必要になります）。

解除するときは、キー名の前に `-` を付けた `.reg` を実行します。

```registry
Windows Registry Editor Version 5.00

[-HKEY_CURRENT_USER\Software\Classes\*\shell\ExtRun]
[-HKEY_CURRENT_USER\Software\Classes\Directory\shell\ExtRun]
```

> [!CAUTION]
> **Windows 11 での注意**: 右クリックメニューが簡略化されているため、追加した項目は「その他のオプションを表示」の中に入ります。Shift + 右クリックで直接開けます。

## 設定ファイル (extrun-config.txt)

実行ファイルと同じフォルダに `extrun-config.txt`（UTF-8）を配置します。メニューは書かれた順に上から下へ表示されます。

同梱の `extrun-config.sample.txt` は書式のほぼすべてを使ったサンプルで、**Windows に最初から入っているコマンドだけで動きます**（画像変換は PowerShell 経由の System.Drawing、書庫の展開は標準の `tar.exe`）。追加のインストールなしでそのまま動かせるので、まず動かしてから、お使いのアプリのパスを書き足していくのが分かりやすいと思います。

**書式の完全な仕様は [extrun-config-format.md](extrun-config-format.md) を参照してください。** ここでは概要だけを示します。

**実際のアプリでどう書くかは [extrun-recipes.md](extrun-recipes.md)（レシピ集）にまとめてあります。** ffmpeg・ImageMagick・IrfanView・7-Zip・VS Code・VLC・Pandoc などの設定例を、それぞれ「どの書式を使っているか」の注記付きで並べてあるので、書式の逆引きとしても使えます。外部アプリを登録するときにつまずきやすい点（コンソールが一瞬で消える、別名が引用符で終わらない、環境変数が展開されない など）も先頭にまとめてあります。

### 基本構造

セクション見出し `[...]` で対象の拡張子を指定し、その下に項目を並べます。項目は `名前 | パス | 引数` の3つのフィールドを `|` で区切って書きます。

```text
[.txt]

メモ帳で開く   | C:\Windows\notepad.exe
VS Code で開く | C:\Program Files\Microsoft VS Code\Code.exe | -n $p
```

パスは絶対パスで書きます。拡張子は先頭の `.` が必須です（`file` と `folder` は例外）。

| 書き方 | 対象 |
| --- | --- |
| `.txt` | 拡張子が txt のファイル |
| `file` | すべてのファイル（フォルダを除く） |
| `folder` | フォルダ |
| `[file folder]` | すべてのファイルとフォルダ |

### 項目ごとの拡張子指定

名前の直後に `[...]` を書くと、その項目だけ対象を変えられます。

```text
[.png .jpg .jpeg .gif .bmp .webp .avif .svg]

JPEG に変換 [-.jpg -.jpeg] | ...   # 継承したものから .jpg / .jpeg を取り除く
アイコンに変換 [.svg]       | ...   # 継承を無視して置き換える
```

### プレースホルダー

引数と作業フォルダで使用できます。

| プレースホルダー | 説明 | 例 |
| --- | --- | --- |
| `$p` | フルパス | `C:\folder\file.txt` |
| `$-p` | 拡張子なしパス | `C:\folder\file` |
| `$d` | 親ディレクトリパス | `C:\folder` |
| `$n` | ファイル名 | `file.txt` |
| `$a` | 拡張子なしファイル名 | `file` |
| `$f` | 親ディレクトリ名 | `folder` |
| `$e` | 拡張子 | `txt` |

引数を省略すると `$p` が渡されます。行末を `|` で終えて引数欄を空にすると、引数なしで起動します。

### 行頭マーカー

名前の前に置きます（後ろに空白が続くときだけマーカーとして働きます）。

| マーカー | 意味 |
| --- | --- |
| `>` `>>` | サブメニューの階層 |
| `+` | 複数選択時、すべてまとめて1プロセスに渡す |

```text
[file folder]

圧縮
> + 7-Zip でまとめて圧縮 | C:\Program Files\7-Zip\7z.exe | a -t7z $d\archive.7z $p -mx9
> ---
> 7-Zip で個別に圧縮     | C:\Program Files\7-Zip\7z.exe | a -t7z $-p.7z $p -mx9
```

`---` だけの行はセパレーターです。先頭・末尾・連続したセパレーターは自動で取り除かれます。

`+` は「複数の入力を並べて受け取れるアプリ」でだけ意味があります（7-Zip の圧縮、ImageMagick の `+append`、VS Code の `--diff`、VLC のプレイリストなど）。逆に ffmpeg の `-i $p` のように入力ごとにオプションが必要なアプリでは意図どおりになりません。具体例は [レシピ集 2-8](extrun-recipes.md#2-8-まとめて渡すが向くもの向かないもの) を参照してください。

すべてのパスが展開されるのは、`$p` を**独立した 1 つの引数**として書いたときだけです。`-i$p` のように他の文字とつなげて書くと最初の 1 つしか渡りません（`--check` が警告します）。引数に `$p` が無い場合は末尾にすべてのパスが追加されます。

### アクセスキー

名前の中の `&` は、次の 1 文字をアクセスキーにします。メニューが出ているあいだにそのキーを押すと、その項目が実行されます。

```text
開く (&O)          「開く (O)」と表示され、O キーで実行
&PNG に変換        「PNG に変換」と表示され、P キーで実行
```

キーはメニューごとに独立しているので、親と子で同じ文字を使えます（`圧縮 (&Z)` → `&ZIP` → `個別に圧縮 (&S)` なら `Z` `Z` `S` の 3 打鍵）。同じメニューの中で重複すると押しても実行されないので、`--check` が警告します。表示したい `&` は `^&` と書きます。

下線が見えないときは Alt キーを押してください。詳細は [extrun-config-format.md](extrun-config-format.md#アクセスキー) を参照してください。

### グローバル設定

`[extrun]` は拡張子の見出しではなく、ExtRun 自体のふるまいを書く場所です。ファイルのどこに書いても全体に効きます。

```text
[extrun]
menu-position = cursor   # cursor / window / screen / X,Y
select-first  = no       # yes にすると先頭の項目を選択した状態で開く
```

**表示位置と初期選択は、コマンドライン引数で上書きできます。** 右クリックから呼ぶときはカーソル位置が正しいのですが、ホットキーから呼ぶときはマウスがどこにあるか分かりません。設定ファイルは 1 つのまま、呼び出しごとに変えられます。

```text
extrun.exe "%1"                              右クリック登録（設定ファイルのまま）
extrun.exe --at window --select-first "%1"   ホットキーから
```

`--at` は `cursor` / `window`（前面ウィンドウの中央）/ `screen`（画面の中央）/ `X,Y`（座標指定）を受け付けます。`--no-select-first` で設定の `yes` を打ち消せます。

### 別名・継続行・作業フォルダ

```text
@apps  = C:\Program Files
@tools = C:\Tools
@7z    = @apps\7-Zip\7z.exe

[.png]

PNG を最適化する
 | @tools\oxipng\oxipng.exe
 | -o max --strip all -a --out $-p_opt.png $p

[folder]

バックアップスクリプトを実行
 | C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe
 | -NoProfile -ExecutionPolicy Bypass -File backup.ps1 -Path "$p"
 :dir @tools\scripts
```

`^` を特殊文字（`$ @ | : > + - # [ ] ^`）の前に置くとエスケープになります。PowerShell のワンライナーを引数に書くときは `^|`（パイプ）・`^$`（PowerShell の変数）・`^@`（配列）が同時に出てくることになります。実例は [レシピ集 2-5](extrun-recipes.md#2-5--と--と--は--でエスケープする) を参照してください。

### 設定の確認

```powershell
extrun.exe --check
```

書式のエラー、別名の重複や未定義、見つからない実行ファイルなどを行番号付きで一覧表示します。同梱のサンプルをそのまま検証すると `問題は見つかりませんでした` と表示されます。

終了コードは、**エラーがあれば 1**、警告だけ・または問題なしなら 0 です。ただし `extrun.exe` はコンソールを持たないアプリとしてビルドされているため、PowerShell は終了を待たずに次へ進みます（`$LASTEXITCODE` は設定されません）。スクリプトから結果を判定するときは次のようにします。

```powershell
# PowerShell
$result = Start-Process .\extrun.exe -ArgumentList '--check' -Wait -PassThru -NoNewWindow
if ($result.ExitCode -ne 0) { throw '設定ファイルにエラーがあります' }
```

```bat
:: コマンドプロンプト / バッチファイルではそのまま待ちます
extrun.exe --check
if errorlevel 1 exit /b 1
```

### 実行される内容の確認

```powershell
extrun.exe --preview "C:\photo\a.jpg"
```

`--check` が**書式**を見るのに対して、`--preview` は**そのパスに対して実際に何が起動されるか**を、起動せずに表示します。プレースホルダーとエスケープが意図どおりに解決されているかを、プロセスを走らせずに確かめられます。

```
対象:
  C:\photo\a.jpg  (.jpg)

形式を変換 (C) > PNG に変換
  実行ファイル  C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe
  引数　　　　  -NoProfile
  引数　　　　  -Command
  引数　　　　  Add-Type -AssemblyName System.Drawing; ...
  作業フォルダ  C:\Windows\System32\WindowsPowerShell\v1.0  （:dir 未指定のため実行ファイルの場所）

15 項目
```

引数は 1 つ 1 行なので、`"..."` で囲み忘れて空白で割れてしまった引数をここで見つけられます。複数のパスを渡すと、個別実行の項目は `[1/2]` のように対象の数だけ並び、`+`（まとめて渡す）の項目は 1 回の起動に全パスが並びます。

## 技術仕様

### 依存クレート

- `windows-sys`: Windows API（ネイティブメニュー表示・コンソール出力）

実行時の依存はこれだけです。設定ファイルのパーサは自前実装で、外部クレートは使用していません。

ビルド時のみ `embed-resource` を使い、exe に VERSIONINFO（プロパティの「詳細」タブに出るバージョンや著作権表記）を埋め込みます。バイナリには含まれず、サイズにも影響しません。リソースのコンパイルには Windows SDK の `rc.exe` が必要です（Visual Studio Build Tools を入れていれば揃っています）。

### パフォーマンス

- **起動速度**: 瞬時（数ミリ秒）
- **メモリ使用量**: 最小限（数MB）
- **最適化**:
  - 起動時のファイル I/O は設定ファイルの読み込み 1 回のみ（実行時の書き込みはゼロ）
  - 構造体ベースのプレースホルダー処理（HashMap 排除）
  - Win32 API 直接呼び出しによる高速メニュー表示

### Windows API 使用

- `CreatePopupMenu`: メニュー作成
- `AppendMenuW`: メニュー項目追加
- `TrackPopupMenu`: メニュー表示とユーザー選択
- `MessageBoxW`: エラーダイアログ表示
- `AttachConsole` / `WriteConsoleW`: `--check` / `--preview` の結果出力

## トラブルシューティング

### メニューが表示されない

- `extrun-config.txt` が `extrun.exe` と同じフォルダにあるか確認
- `extrun.exe --check` で書式のエラーを確認
- 設定ファイルが UTF-8 で保存されているか確認（Shift-JIS では読めません）

### アプリが起動しない

起動できなかったときは、理由を書いたエラーダイアログが出ます。それを見ても分からないときは次を確認してください。

- `extrun.exe --check` で実行ファイルのパスを確認
- パスが絶対パスになっているか確認（相対パスは起動元の作業フォルダ基準になります）
- `.bat` / `.cmd` / `.ps1` は直接起動できません。`cmd.exe /c` や `powershell -File` を経由してください
- `:dir` で指定した作業フォルダが実在するか確認

### 初回起動時に SmartScreen の警告が出る

配布している `extrun.exe` はコード署名をしていないため、「Windows によって PC が保護されました」という警告が出ることがあります。続行する場合は「詳細情報」→「実行」を選んでください。ダウンロードした zip が壊れていないかは、同梱の `.sha256` と照合して確認できます。

```powershell
Get-FileHash .\extrun-<version>-win-x64.zip -Algorithm SHA256
```

同じ理由で、ウイルス対策ソフトが未署名の実行ファイルを警告することがあります。気になる場合はソースからビルドしてください（`cargo build --release`）。

### コンソールが一瞬で開いて消える／結果が読めない

ffmpeg や 7z.exe のようなコンソールアプリを直接起動すると、処理が終わった瞬間にウィンドウごと閉じるため、エラーメッセージが残りません。結果を読みたい項目は PowerShell を挟んで `-NoExit` を付けてください。書き方は [レシピ集 2-1](extrun-recipes.md#2-1-黒い窓が一瞬で消えて結果が見えない) にあります。

## セキュリティについて

ExtRun は、`extrun-config.txt` に書かれたコマンドをそのまま起動するツールです。**設定ファイルは実行可能なスクリプトと同じもの**だと考えてください。

- 出所の分からない `extrun-config.txt` をそのまま使わないでください。中身を読んでから使ってください。
- `extrun.exe` は自分と同じフォルダの設定ファイルだけを読みます。誰でも書き込めるフォルダ（`C:\` 直下など）に置くと、他のユーザーやプログラムに設定を書き換えられる可能性があります。`C:\Tools\extrun\` のような、書き込み権限が管理された場所に置いてください。
- ExtRun 自身は管理者権限を必要とせず、レジストリの編集も、設定ファイル以外のファイル I/O も行いません。

## ライセンス

MIT License — 詳細は [LICENSE](LICENSE) を参照してください。

変更履歴は [CHANGELOG.md](CHANGELOG.md) にあります。

## 開発

### ビルド環境

- Rust 2021 Edition（`rust-version = 1.77`）
- Windows 11 / 10
- Windows SDK の `rc.exe`（VERSIONINFO の埋め込みに使用）

```powershell
cargo build --release
cargo test
cargo clippy --all-targets
cargo fmt --check
```

同じ内容を GitHub Actions（[.github/workflows/ci.yml](.github/workflows/ci.yml)）が `windows-latest` で実行します。テストは `extrun-config.txt` をフィクスチャとして読むので、サンプル設定の書式エラーも CI で検出されます。

### リリース用パッケージの作成

```powershell
.\packaging\build-release.ps1
```

テスト → リリースビルド → 配布物の組み立て → zip → SHA256 を通しで実行し、`dist\` に出力します。バージョンは `Cargo.toml` から読みます。上げるときに手で直すのは `Cargo.toml` と `packaging\readme.txt` の見出し、`CHANGELOG.md` の 3 か所で、ずれているとビルドスクリプトが止まります。

`v1.2.3` のようなタグを push すると、[.github/workflows/release.yml](.github/workflows/release.yml) が同じスクリプトを実行して、zip と `.sha256` を GitHub Releases に添付します。

```text
dist/
├── extrun-<version>-win-x64.zip
└── extrun-<version>-win-x64.zip.sha256

zip の中身:
extrun-<version>/
├── extrun.exe
├── readme.txt                 # packaging/readme.txt。配布専用でこの README とは別物
├── extrun-config.sample.txt   # extrun-config.txt をリネームしたもの
├── extrun-config-format.md
├── extrun-recipes.md
├── CHANGELOG.md
├── registry/                  # 右クリックメニュー登録用（UTF-16 LE に変換して同梱）
│   ├── extrun-add.reg
│   └── extrun-remove.reg
└── LICENSE
```

### プロジェクト構成

```text
extrun/
├── src/
│   ├── main.rs         # エントリポイント、引数処理
│   ├── config.rs       # 設定ファイルのパース
│   ├── check.rs        # --check の検証と整形
│   ├── preview.rs      # --preview の整形
│   ├── console.rs      # コンソールへの出力
│   ├── menu.rs         # Win32メニュー作成・表示・実行
│   └── placeholder.rs  # プレースホルダー置換
├── packaging/
│   ├── build-release.ps1       # 配布用 zip の作成
│   └── readme.txt              # 配布物に同梱する説明書
├── .github/workflows/
│   ├── ci.yml                  # fmt / clippy / test
│   └── release.yml             # タグから zip を作って Release に添付
├── docs/images/                # README で使う画像
├── build.rs                    # ビルド設定（サブシステムと VERSIONINFO）
├── extrun-config.txt           # 設定ファイル（サンプル兼テスト用フィクスチャ）
├── extrun-config-format.md     # 設定ファイルの仕様
├── extrun-recipes.md           # 外部アプリを使った設定例集
├── Cargo.toml                  # 依存関係
├── CHANGELOG.md                # 変更履歴
├── LICENSE                     # MIT License
└── README.md                   # このファイル
```
