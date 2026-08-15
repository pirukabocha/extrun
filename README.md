# ExtRun

**ExtRun** は、拡張子に関連付けられたコンテキストメニューから、ファイルやフォルダを任意のアプリで開く Windows 用ランチャーです。

> **In English** — ExtRun is a tiny Windows launcher written in Rust. Pass it file or folder paths and it pops up a context menu at the cursor, showing only the commands that apply to those file types, then spawns the one you pick and exits. Menus are defined in a single plain-text config file (one line per entry) next to the executable. No installer, no background process, no registry writes. Documentation is in Japanese; the config file format is summarized under [設定ファイル](#設定ファイル-extrun-configtxt) and specified in full in [extrun-config-format.md](docs/extrun-config-format.md).

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
4. 任意で「送る」メニューに登録（下記）

同梱のサンプル設定は Windows 標準のコマンドだけで動くので、追加のインストールなしでそのまま試せます。

設定ファイルが `extrun-config.sample.txt` という名前で入っているのは、更新版を同じフォルダに展開したときに、書き換えた `extrun-config.txt` を上書きで消さないためです。

ffmpeg・7-Zip・ImageMagick・VS Code といった手持ちのアプリを登録する段階になったら、[extrun-recipes.md](docs/extrun-recipes.md)（レシピ集）にそのまま貼って使える設定例をまとめてあります。

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

## 設定ファイル (extrun-config.txt)

実行ファイルと同じフォルダに `extrun-config.txt`（UTF-8）を配置します。メニューは書かれた順に上から下へ表示されます。

同梱の `extrun-config.sample.txt` は書式のほぼすべてを使ったサンプルで、**Windows に最初から入っているコマンドだけで動きます**（画像変換は PowerShell 経由の System.Drawing、書庫の展開は標準の `tar.exe`）。追加のインストールなしでそのまま動かせるので、まず動かしてから、お使いのアプリのパスを書き足していくのが分かりやすいと思います。

**書式の完全な仕様は [extrun-config-format.md](docs/extrun-config-format.md) を参照してください。** ここでは概要だけを示します。

**実際のアプリでどう書くかは [extrun-recipes.md](docs/extrun-recipes.md)（レシピ集）にまとめてあります。** ffmpeg・ImageMagick・IrfanView・7-Zip・VS Code・VLC・Pandoc などの設定例を、それぞれ「どの書式を使っているか」の注記付きで並べてあるので、書式の逆引きとしても使えます。外部アプリを登録するときにつまずきやすい点（コンソールが一瞬で消える、別名が引用符で終わらない、環境変数が展開されない など）も先頭にまとめてあります。

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

### 日時

`$t{...}` は実行した日時に置き換わります。上書きを避けたいときや、同じ操作を日をまたいで繰り返すときに使えます。

```text
バックアップ | C:\Windows\System32\tar.exe | -a -c -f $-p_$t{yyyyMMdd}.zip -C $d $n
```

| 書式 | 内容 | 例（2026-08-15 土曜 14:03:05） |
| --- | --- | --- |
| `yyyy` / `yy` | 年 | `2026` / `26` |
| `MM` / `M` | 月（大文字） | `08` / `8` |
| `dd` / `d` | 日 | `15` |
| `HH` / `H` | 時（24 時間制） | `14` |
| `mm` / `m` | 分（小文字） | `03` / `3` |
| `ss` / `s` | 秒 | `05` / `5` |
| `ddd` / `dddd` | 曜日（日本語） | `土` / `土曜日` |
| `EEE` / `EEEE` | 曜日（英語） | `Sat` / `Saturday` |

英字でない文字はそのまま通るので、`$t{yyyy年MM月dd日}` や `$t{yyyy-MM-dd}` と書けます。**`d` は文字数で意味が変わり**、`d` `dd` は日、`ddd` `dddd` は曜日です。

書式以外の英字を中括弧に入れることはできません（`$t{yyyyMMdd_backup}` ではなく `$t{yyyyMMdd}_backup` と書きます）。書き間違いは `--check` がエラーとして報告します。

時刻は**項目を選んだ時点**で 1 回だけ決まり、その実行の中ではすべての対象で同じ値になります。複数ファイルを選んで個別に起動しても、`$t{ss}` の秒が食い違ってファイル名がばらけることはありません。

### 入力欄

`$?{...}` は、項目を選んだあとに入力欄を出して、入力された文字に置き換わります。値をひとつ変えたいだけの項目を、解像度や品質の数だけ並べずに済みます。

```text
長辺を指定して縮小 | C:\Tools\magick.exe | $p -resize $?{長辺のピクセル数=1280}x$?{長辺のピクセル数=1280}^> $-p_small.png
```

中括弧の中は `説明=既定値` の形で、最初の `=` で分かれます。既定値は省略できます。

- **既定値は選択された状態で入る**ので、そのまま打てば置き換わり、Enter だけなら既定値が使われます
- **同じ内容の `$?{...}` は 1 回しか聞かれません**。上の例のように、入力した値を複数の場所で使えます
- **キャンセルすると何も実行されません**
- 説明と既定値の中ではプレースホルダーと日時が使えます（`$?{$n の新しい名前=$a_$t{yyyyMMdd}}`）
- `$?` は中括弧が続いたときだけ入力欄になるので、PowerShell の `$?` はそのまま渡ります

順番は**入力欄 → 確認（`:confirm`）→ 起動**です。確認のメッセージにも `$?{...}` を書けるので、入力した値を確かめてから実行できます。

引数は空白で区切られますが、`$?{...}` と `$t{...}` の中括弧の中は区切られません。`$?{$n の新しい名前}` のように空白を含む説明をそのまま書けます。

#### 入力値の決まり

`$?` と `{` のあいだに書くと、入力できる値を絞れます。

| 書き方 | 受け付ける値 |
| --- | --- |
| `$?{...}` | 制限なし |
| `$?int{...}` | 整数（`1280` `-5`） |
| `$?num{...}` | 数値（`1.5` `-2`） |
| `$?name{...}` | ファイル名に使える文字 |

```text
長辺を指定して縮小 | C:\Tools\magick.exe | $p -resize $?int{長辺のピクセル数=1280} $-p_small.png
```

**決まりを満たさない入力は、打ち切らずに聞き直します。** 理由を添えて、打った内容が残った状態で入力欄が出直ります。

`$?int` は全角数字（`１２８０`）や単位付き（`1280px`）も弾きます。`$?name` は空・使えない文字（`\ / : * ? " < > |`）・終わりの `.` と空白・予約された装置名（`CON` `NUL` など）・255 文字超（NTFS の名前 1 要素の上限）を弾きます。ただし**パス全体の長さは見ません** — 入力値がパスのどこに入るかを ExtRun は知らないためです。

**既定値も `--check` が検証します。** `$?int{品質=たかい}` のような書き間違いは実行前に見つかります。

正規表現や文字数の指定は用意していません（正規表現エンジンは依存クレートが増えるため）。それ以外の細かい制約は `:confirm` で入力値を見せて確かめる形になります。

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

`+` は「複数の入力を並べて受け取れるアプリ」でだけ意味があります（7-Zip の圧縮、ImageMagick の `+append`、VS Code の `--diff`、VLC のプレイリストなど）。逆に ffmpeg の `-i $p` のように入力ごとにオプションが必要なアプリでは意図どおりになりません。具体例は [レシピ集 2-8](docs/extrun-recipes.md#2-8-まとめて渡すが向くもの向かないもの) を参照してください。

すべてのパスが展開されるのは、`$p` を**独立した 1 つの引数**として書いたときだけです。`-i$p` のように他の文字とつなげて書くと最初の 1 つしか渡りません（`--check` が警告します）。引数に `$p` が無い場合は末尾にすべてのパスが追加されます。

### アクセスキー

名前の中の `&` は、次の 1 文字をアクセスキーにします。メニューが出ているあいだにそのキーを押すと、その項目が実行されます。

```text
開く (&O)          「開く (O)」と表示され、O キーで実行
&PNG に変換        「PNG に変換」と表示され、P キーで実行
```

キーはメニューごとに独立しているので、親と子で同じ文字を使えます（`圧縮 (&Z)` → `&ZIP` → `個別に圧縮 (&S)` なら `Z` `Z` `S` の 3 打鍵）。同じメニューの中で重複すると押しても実行されないので、`--check` が警告します。表示したい `&` は `^&` と書きます。

下線が見えないときは Alt キーを押してください。詳細は [extrun-config-format.md](docs/extrun-config-format.md#アクセスキー) を参照してください。

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

`^` を特殊文字（`$ @ | : > + - # [ ] ^`）の前に置くとエスケープになります。PowerShell のワンライナーを引数に書くときは `^|`（パイプ）・`^$`（PowerShell の変数）・`^@`（配列）が同時に出てくることになります。実例は [レシピ集 2-5](docs/extrun-recipes.md#2-5--と--と--は--でエスケープする) を参照してください。

### 実行前の確認

取り消しのきかない操作には `:confirm` を付けられます。選んだあと・起動する前に確認のダイアログが出て、「いいえ」を選ぶと何も起きません。

```text
完全に削除する
 | C:\Windows\System32\cmd.exe
 | /c del /f /q "$p"
 :confirm $n を削除します。ごみ箱には入りません。
```

メッセージは省略できます（`:confirm` とだけ書くと項目名を使った文面になります）。ダイアログには**対象の一覧が必ず添えられる**ので、選び間違えていないかを目で確かめられます。

**既定のボタンは「いいえ」です。** `select-first` を有効にしていると Enter だけで実行まで進めてしまいますが、確認を挟めばそのまま Enter を続けても止まります。確認は項目に対して 1 回だけで、複数のファイルを選んでも対象の数だけ聞かれることはありません。

### アイコン

`:icon` を書いた項目には、名前の左にアイコンが出ます。

```text
開く (&O)
 :icon C:\Windows\System32\imageres.dll,3
> 既定のアプリで開く (&D) | C:\Windows\explorer.exe | $p
```

値は `パス` または `パス,番号` です。番号は dll や exe に複数のアイコンが入っているときに選びます（`.reg` やショートカットと同じ書き方）。Windows 標準のアイコンは `imageres.dll` と `shell32.dll` にまとまっています。**サブメニューの親にも付きます。**

出すかどうかは `[extrun]` の `icons` で切り替えます。

| 値 | 動き |
| --- | --- |
| `none` | 出さない（`:icon` を書いていても無視） |
| `specified` | `:icon` を書いた項目だけ出す（**既定**） |
| `auto` | `:icon` を優先し、書いていない項目は実行ファイルから取り出す |

既定が `specified` なので、`:icon` を 1 つも書いていない設定ではアイコンの読み込みが一切走らず、**起動時間はこれまでと変わりません**。`none` は「設定は残したまま、しばらく出したくない」ときのためのものです。

`auto` は項目ごとに実行ファイルからアイコンを取り出すぶん、メニューが出るまでが少し遅くなります。同じ実行ファイルは 1 度しか読みませんが、そのぶん PowerShell 経由の項目は全部同じアイコンになります。見分けたい項目に `:icon` を書く方が効果的です。

`--check` は実際に取り出せるかまで確かめ、ファイルが無いときも番号が範囲の外のときも警告します（どちらも警告なので、メニュー自体は表示されます）。

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

ffmpeg や 7z.exe のようなコンソールアプリを直接起動すると、処理が終わった瞬間にウィンドウごと閉じるため、エラーメッセージが残りません。結果を読みたい項目は PowerShell を挟んで `-NoExit` を付けてください。書き方は [レシピ集 2-1](docs/extrun-recipes.md#2-1-黒い窓が一瞬で消えて結果が見えない) にあります。

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
├── extrun-config-format.md    # docs/ から。zip の中ではフラットに並べる
├── extrun-recipes.md          # 同上
├── CHANGELOG.md
└── LICENSE
```

### プロジェクト構成

```text
extrun/
├── src/
│   ├── main.rs         # エントリポイント、引数処理
│   ├── config.rs       # 設定ファイルのパース
│   ├── menu.rs         # Win32 メニュー作成・表示・実行
│   ├── placeholder.rs  # プレースホルダー置換
│   ├── datetime.rs     # $t{...} の書式解釈
│   ├── prompt.rs       # $?{...} の入力ダイアログ
│   ├── icon.rs         # :icon のアイコン取り出し
│   ├── check.rs        # --check の検証と整形
│   ├── preview.rs      # --preview の整形
│   └── console.rs      # コンソールへの出力
├── docs/
│   ├── extrun-config-format.md # 設定ファイルの仕様
│   ├── extrun-recipes.md       # 外部アプリを使った設定例集
│   └── images/                 # README で使う画像
├── packaging/
│   ├── build-release.ps1       # 配布用 zip の作成
│   └── readme.txt              # 配布物に同梱する説明書
├── .github/workflows/
│   ├── ci.yml                  # fmt / clippy / test
│   └── release.yml             # タグから zip を作って Release に添付
├── build.rs                    # ビルド設定（サブシステムと VERSIONINFO）
├── extrun-config.txt           # 設定ファイル（サンプル兼テスト用フィクスチャ）
├── Cargo.toml                  # 依存関係
├── CHANGELOG.md                # 変更履歴
├── LICENSE                     # MIT License
└── README.md                   # このファイル
```
