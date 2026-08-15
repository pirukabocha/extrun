# ExtRun

**ExtRun** は、拡張子に関連付けられたコンテキストメニューから、ファイルやフォルダを任意のアプリで開く Windows 用ランチャーです。

> **In English** — ExtRun is a tiny Windows launcher written in Rust. Pass it file or folder paths and it pops up a context menu at the cursor, showing only the commands that apply to those file types, then spawns the one you pick and exits. Menus are defined in a single plain-text config file (one line per entry) next to the executable. No installer, no background process, no registry writes. Documentation is in Japanese; the config file format is summarized under [設定ファイル](#設定ファイル) and specified in full in [docs/extrun-config-format.md](docs/extrun-config-format.md).

![ExtRun のメニュー](docs/images/menu.png)

## 特徴

- **⚡ 超高速起動**: Rust 製ネイティブアプリケーション
- **🎯 拡張子ベースフィルタリング**: ファイルの種類に応じて適切なアプリだけを表示
- **📁 複数ファイル対応**: 一度に複数のファイル/フォルダを処理可能
- **🔧 柔軟な設定**: 1行1項目のシンプルな設定ファイル
- **🖱️ マウスとキーボードの両方に配慮**: ユーザーの入力環境を問わず利用しやすい操作感
- **💾 省メモリ**: 実行時の依存クレートは `windows-sys` のみ

## インストール

1. リリースページから `extrun-<version>-win-x64.zip` をダウンロード
2. 任意のフォルダに展開
3. `extrun-config.sample.txt` をコピーして `extrun-config.txt` にリネーム
4. 任意で「送る」メニューに登録（[下記](#windows-エクスプローラから使う)）

同梱のサンプル設定は Windows 標準のコマンドだけで動くので、追加のインストールなしでそのまま試せます。

設定ファイルが `extrun-config.sample.txt` という名前で入っているのは、更新版を同じフォルダに展開したときに、書き換えた `extrun-config.txt` を上書きで消さないためです。

ソースからビルドする場合は `cargo build --release` です。詳しくは [docs/development.md](docs/development.md) を参照してください。

## 使用方法

```powershell
# ファイル / フォルダのパスを引数で渡すとメニューが出る
extrun.exe document.txt
extrun.exe image1.jpg image2.jpg image3.jpg
extrun.exe C:\Projects\MyProject

# 設定ファイルを検証する
extrun.exe --check

# 実際に起動されるコマンドラインを、起動せずに表示する
extrun.exe --preview image.jpg

extrun.exe --version
extrun.exe --help
```

メニューは既定でカーソル位置に出ます。`--at` / `--select-first` で呼び出しごとに変えられます（[グローバル設定](docs/extrun-config-format.md#グローバル設定)）。

### Windows エクスプローラから使う

> [!NOTE]
> AutoHotkey ユーザー向けに、便利なスクリプトを [extrun-recipes.md](docs/extrun-recipes.md#付録-c-autohotkey-から呼び出す) に付録として記載していますので、そちらも参考にして下さい。

**「送る」メニューに登録するのがおすすめです。**

1. エクスプローラのアドレスバーに `shell:sendto` と入力
2. 開いたフォルダに `extrun.exe` のショートカットを置く

右クリック →「送る」→ ExtRun で、選択中のファイルにメニューが出ます。**選んだファイルは何個でも 1 つの ExtRun にまとめて渡される**ので、`+`（まとめて渡す）を付けた項目もそのまま使えます。

ショートカットは何個でも置けます。「リンク先」の末尾にオプションを足しておけば、呼び出し方ごとにメニューの出方を変えられます。

```text
shell:sendto\
├── ExtRun.lnk           "C:\Tools\extrun\extrun.exe"
└── ExtRun (中央).lnk    "C:\Tools\extrun\extrun.exe" --at screen --select-first
```

> [!NOTE]
> **Windows 11 では「送る」は「その他のオプションを表示」の中にあります。** Shift + 右クリックで直接開けます。

#### 右クリックメニューへの直接登録は勧めません

レジストリ（`HKCU\Software\Classes\*\shell\...`）に項目を足せば、「送る」を経由せず右クリックメニューへ直接出せます。ver. 1.1.0 まではこの `.reg` を配布 zip に同梱していましたが、**Windows の仕様上の制限が大きいため取りやめました**。

`shell\...\command` に `"%1"` を書く形式（legacy 動詞）には次の制限があります。

- **複数のファイルを選ぶと、ファイルの数だけプロセスが起動します。** `+`（まとめて渡す）が原理的に機能しません
- **16 個以上のファイルを選ぶと、右クリックメニューから項目自体が消えます。** [MultiSelectModel](https://learn.microsoft.com/en-us/windows/win32/shell/how-to-employ-the-verb-selection-model) の既定（`Document`）の上限で、`Player` を指定しても上限が 100 個に変わるだけです。呼び出しが 1 回にまとまるわけではありません

1 回の起動で全パスを受け取るには COM の DropTarget / ExecuteCommand ハンドラが必要で、「常駐しない・レジストリを書かない・実行時の依存は `windows-sys` のみ」という ExtRun の設計とは両立しません。「送る」の SendTo ハンドラはこの COM 実装なので、そちらを使えば制限を受けません。

ver. 1.1.0 までの `extrun-add.reg` で登録済みの場合は、同じ zip に入っていた `extrun-remove.reg` で解除できます。手元に無いときは PowerShell で次を実行してください。

```powershell
Remove-Item -LiteralPath 'HKCU:\Software\Classes\*\shell\ExtRun' -Recurse
Remove-Item -LiteralPath 'HKCU:\Software\Classes\Directory\shell\ExtRun' -Recurse
```

`-LiteralPath` が要るのは、`*` を PowerShell がワイルドカードとして解釈しないようにするためです。

## 設定ファイル

実行ファイルと同じフォルダに `extrun-config.txt`（UTF-8）を置きます。1 行 1 項目で、`名前 | パス | 引数` を `|` で区切って書きます。メニューは書かれた順に上から下へ表示されます。

```text
[.txt]

メモ帳で開く   | C:\Windows\notepad.exe
VS Code で開く | C:\Program Files\Microsoft VS Code\Code.exe | -n $p
```

`[.txt]` は「ここから下は `.txt` が対象」という見出しです（`file` はすべてのファイル、`folder` はフォルダ）。パスは絶対パスで書き、`$p` は選んだファイルのフルパスに置き換わります。

このほかに書けるものは次のとおりです。

| 書けること | 記法 | 詳細 |
| --- | --- | --- |
| パスの部品 | `$p` `$-p` `$d` `$n` `$a` `$f` `$e` | [プレースホルダー](docs/extrun-config-format.md#プレースホルダー) |
| 実行した日時 | `$t{yyyyMMdd}` `$t{HH-mm-ss}` `$t{ddd}` | [日時](docs/extrun-config-format.md#日時) |
| 選んだあとに値を聞く | `$?{説明=既定値}` `$?int{...}` `$?name{...}` | [入力欄](docs/extrun-config-format.md#入力欄) |
| 実行前の確認ダイアログ | `:confirm メッセージ` | [実行前の確認](docs/extrun-config-format.md#実行前の確認) |
| 項目にアイコンを出す | `:icon パス,番号` | [アイコン](docs/extrun-config-format.md#アイコン) |
| サブメニュー・区切り線 | `>` `>>` `---` | [行頭マーカー](docs/extrun-config-format.md#行頭マーカー) |
| 複数選択をまとめて渡す | `+` | [行頭マーカー](docs/extrun-config-format.md#行頭マーカー) |
| キーボードで選ぶ | `開く (&O)` | [アクセスキー](docs/extrun-config-format.md#アクセスキー) |
| 項目ごとに対象を変える | `[-.jpg]` `[.svg]` | [項目ごとの指定](docs/extrun-config-format.md#項目ごとの指定) |
| パスの共通化・作業フォルダ | `@名前 = 値` `:dir` | [別名](docs/extrun-config-format.md#別名) |
| メニューの位置・アイコンの有無 | `[extrun]` | [グローバル設定](docs/extrun-config-format.md#グローバル設定) |

**書式の完全な仕様は [docs/extrun-config-format.md](docs/extrun-config-format.md) が正典です。** 巻頭に記法の早見表と目次があります。

**実際のアプリでどう書くかは [docs/extrun-recipes.md](docs/extrun-recipes.md)（レシピ集）にまとめてあります。** ffmpeg・ImageMagick・IrfanView・7-Zip・VS Code・VLC・Pandoc などの設定例を、それぞれ「どの書式を使っているか」の注記付きで並べてあるので、書式の逆引きとしても使えます。外部アプリを登録するときにつまずきやすい点（コンソールが一瞬で消える、別名が引用符で終わらない、環境変数が展開されない など）も先頭にまとめてあります。

同梱の `extrun-config.sample.txt` は書式のほぼすべてを使ったサンプルで、**Windows に最初から入っているコマンドだけで動きます**（画像変換は PowerShell 経由の System.Drawing、書庫の展開は標準の `tar.exe`）。まず動かしてから、お使いのアプリのパスを書き足していくのが分かりやすいと思います。

### 書き換えたら確かめる

```powershell
extrun.exe --check                       # 書式・別名・実行ファイルのパスを検証
extrun.exe --preview "C:\photo\a.jpg"    # 起動せずにコマンドラインを表示
```

`--check` が**書式**を見るのに対して、`--preview` は**そのパスに対して実際に何が起動されるか**を見せます。引数は 1 つ 1 行で表示されるので、`"..."` で囲み忘れて空白で割れた引数を見つけられます。

```text
形式を変換 (C) > PNG に変換
  実行ファイル  C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe
  引数　　　　  -NoProfile
  引数　　　　  -Command
  引数　　　　  Add-Type -AssemblyName System.Drawing; ...
  作業フォルダ  C:\Windows\System32\WindowsPowerShell\v1.0  （:dir 未指定のため実行ファイルの場所）
```

`--check` の終了コードは、エラーがあれば 1、警告だけ・または問題なしなら 0 です。ただし `extrun.exe` はコンソールを持たないアプリとしてビルドされているため、**PowerShell は終了を待ちません**（`$LASTEXITCODE` は設定されません）。スクリプトから判定するときは `Start-Process -Wait -PassThru` の `ExitCode` を使ってください。

## 困ったときは

| 症状 | 確認すること |
| --- | --- |
| メニューが表示されない | `extrun-config.txt` が `extrun.exe` と同じフォルダにあるか / `--check` で書式エラーが無いか / UTF-8 で保存されているか（Shift-JIS は読めません） |
| アプリが起動しない | 起動失敗の理由はダイアログに出ます。`--check` で実行ファイルのパスを確認。パスは絶対パスで。`.bat` / `.cmd` / `.ps1` は直接起動できないので `cmd.exe /c` や `powershell -File` を経由してください |
| 選べる項目が少ない | その拡張子に対応する項目が設定に書かれていない可能性があります |
| コンソールが一瞬で消えて結果が読めない | PowerShell を挟んで `-NoExit` を付けます（[レシピ集 2-1](docs/extrun-recipes.md#2-1-黒い窓が一瞬で消えて結果が見えない)） |
| SmartScreen の警告が出る | 配布している `extrun.exe` はコード署名をしていないためです。「詳細情報」→「実行」で続行できます。zip が壊れていないかは同梱の `.sha256` と `Get-FileHash` の結果を照合して確認できます |

## セキュリティについて

ExtRun は、`extrun-config.txt` に書かれたコマンドをそのまま起動するツールです。**設定ファイルは実行可能なスクリプトと同じもの**だと考えてください。

- 出所の分からない `extrun-config.txt` をそのまま使わないでください。中身を読んでから使ってください。
- `extrun.exe` は自分と同じフォルダの設定ファイルだけを読みます。誰でも書き込めるフォルダ（`C:\` 直下など）に置くと、他のユーザーやプログラムに設定を書き換えられる可能性があります。`C:\Tools\extrun\` のような、書き込み権限が管理された場所に置いてください。
- ExtRun 自身は管理者権限を必要とせず、**レジストリの編集も、設定ファイル以外のファイル I/O も行いません。**

## ドキュメント

| | |
| --- | --- |
| [docs/extrun-config-format.md](docs/extrun-config-format.md) | 設定ファイル形式の完全な仕様（記法の早見表つき） |
| [docs/extrun-recipes.md](docs/extrun-recipes.md) | 外部アプリを使った設定例集と、AutoHotkey から呼び出す例 |
| [docs/development.md](docs/development.md) | ビルド環境・プロジェクト構成・リリース手順 |
| [CHANGELOG.md](CHANGELOG.md) | 変更履歴 |

## ライセンス

MIT License — 詳細は [LICENSE](LICENSE) を参照してください。
