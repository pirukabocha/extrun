# ExtRun レシピ集

同梱のサンプル設定 `extrun-config.txt` は「追加インストールなしで動く」ことを優先したため、Windows 標準のコマンドだけで書かれています。このドキュメントは、そこから外した**実際に使っているアプリの設定例**をまとめたものです。

書式そのものの仕様は `extrun-config-format.md` を参照してください。ここでは「よくあるアプリを、どう書くか」だけを扱います。

> [!IMPORTANT]
> **使う前に**
>
> - **パスは自分の環境に読み替えてください。** インストーラ・winget・scoop・zip 展開のどれで入れたかでパスは変わります。
> - レシピをコピーしたら `extrun.exe --check` で書式とパスを確かめ、**まず 1 ファイルで試してから**常用してください。
> - 変換系のレシピは、すべて**元のファイルを書き換えず、新しいファイルを作る**書き方に統一してあります。

---

## 目次

- [1. レシピの読み方](#1-レシピの読み方)
- [2. つまずきやすいところ](#2-つまずきやすいところ)
- [3. ffmpeg — 動画・音声](#3-ffmpeg--動画音声)
- [4. ImageMagick / IrfanView — 画像](#4-imagemagick--irfanview--画像)
- [5. 7-Zip — 圧縮・展開](#5-7-zip--圧縮展開)
- [6. VS Code — 開く・比べる](#6-vs-code--開く比べる)
- [7. VLC — 再生する](#7-vlc--再生する)
- [8. ターミナル / WSL / Git — フォルダで作業を始める](#8-ターミナル--wsl--git--フォルダで作業を始める)
- [9. Pandoc — 文書を変換する](#9-pandoc--文書を変換する)
- [10. 画像最適化 — oxipng / pngquant / cwebp](#10-画像最適化--oxipng--pngquant--cwebp)
- [付録 A. 書式の逆引き](#付録-a-書式の逆引き)
- [付録 B. 別名まとめ（コピペ用）](#付録-b-別名まとめコピペ用)
- [付録 C. AutoHotkey から呼び出す](#付録-c-autohotkey-から呼び出す)

---

## 1. レシピの読み方

### 1-1. レシピは 3 つの型しかない

アプリが何であれ、設定の書き方は結局この 3 つのどれかになります。ここに載っていないアプリを追加するときも、まずどの型かを考えると早いです。

**型 1 — GUI アプリにパスを渡すだけ**

```
VLC で再生 | @vlc | $p
```

引数を省略すると `$p` が渡されるので、`| @vlc` だけでも同じです。「関連付けを変えずに、複数の開き方を並べる」のが ExtRun の一番素朴な使い方です。

**型 2 — CLI を直接呼ぶ**

```
MP4 に変換 [-.mp4] | @ffmpeg | -n -i $p -c:v libx264 -crf 23 -c:a aac $-p.mp4
```

`$-p`（拡張子を除いたフルパス）で出力先を組み立てるのが基本形。**このとき起動したコンソールは終了と同時に消える**ので、失敗しても何も残りません（→ [2-1](#2-1-黒い窓が一瞬で消えて結果が見えない)）。

**型 3 — PowerShell でラップする**

```
情報を表示
 | @powershell
 | -NoProfile -NoExit -Command "& '@ff\ffprobe.exe' -v error -show_format '$p'"
```

結果を画面に残したい、複数のコマンドをつなげたい、出力を加工したい場合はこの型。ExtRun のレシピで一番よく使う形です。

### 1-2. 実行ファイルのパスを調べる

**パスは絶対パスで書く必要があります。** PowerShell で次のように調べられます。

```powershell
where.exe ffmpeg                      # PATH が通っていれば一発
(Get-Command ffmpeg).Source           # 同上
Get-ChildItem 'C:\Program Files' -Recurse -Filter '7z.exe' -ErrorAction SilentlyContinue
```

winget で入れたアプリは `%LOCALAPPDATA%\Microsoft\WinGet\Links\` に、scoop なら `%USERPROFILE%\scoop\shims\` にリンクが置かれます。ただしこれらは実体ではなくシムなので、**実体のパスを書いたほうが確実**です。

### 1-3. 別名で「直す場所」を 1 か所にまとめる

外部アプリを使うレシピの最大の面倒は、インストール先が人によって違うことです。ファイルの先頭に別名を集めておけば、環境が変わったときに直すのはそこだけで済みます。

```
@apps  = C:\Program Files
@tools = C:\Tools

@ff       = @tools\ffmpeg\bin
@ffmpeg   = @ff\ffmpeg.exe
@7z       = @apps\7-Zip\7z.exe
@magick   = @apps\ImageMagick\magick.exe
```

拡張子のまとまりも同じように名前を付けられます。以降のレシピはすべてこの別名を使います。

```
@画像 = .png .jpg .jpeg .gif .bmp .tif .tiff .webp .avif
@動画 = .mp4 .mkv .avi .wmv .mov .flv .webm .ts .m2ts
@音声 = .wav .mp3 .m4a .aac .flac .wma .opus .ogg
@書庫 = .zip .7z .rar .tar .gz .tgz .bz2 .xz .lzh .cab .iso
```

---

## 2. つまずきやすいところ

外部アプリを使い始めると、ほぼ全員が同じところでつまずきます。先に目を通しておくと早いです。

### 2-1. 黒い窓が一瞬で消えて結果が見えない

ffmpeg や 7z.exe のようなコンソールアプリを直接起動すると、処理が終わった瞬間にウィンドウごと消えます。成功すれば出力ファイルができているので分かりますが、**失敗したときは何も分かりません**。

結果を読みたいレシピは PowerShell でラップし、`-NoExit` を付けます。

```
MP4 に変換（結果を確認する） [-.mp4]
 | @powershell
 | -NoProfile -NoExit -Command "& '@ff\ffmpeg.exe' -n -i '$p' -c:v libx264 -crf 23 -c:a aac '$-p.mp4'"
```

- `-NoProfile` — プロファイルの読み込みを飛ばして起動を速くする
- `-NoExit` — コマンドが終わってもウィンドウを閉じない
- `& '実行ファイル'` — パスに空白があっても動くように呼び出し演算子を使う

普段は直接起動の軽い版を使い、うまくいかないときだけラップ版に差し替える、という運用でも構いません。

### 2-2. 別名は引用符では終わらない

`@` の後ろは **空白 `\` `|` `[` `]` `^` `@` 行末** のいずれかが現れるまでが名前です。**`'` や `"` は終端になりません。**

```
NG   & '@ffmpeg' -i '$p' ...      →  ffmpeg' という別名を探して「未定義」エラー
OK   & '@ff\ffmpeg.exe' -i '$p' ...
```

PowerShell でラップするレシピでは実行ファイルを `'...'` で囲むことになるので、**必ず `\` を含む形の別名**（`@ff\ffmpeg.exe` や `@apps\7-Zip\7z.exe`）にします。このために、ffmpeg の別名はフォルダ用の `@ff` と実行ファイル用の `@ffmpeg` を両方定義しておくと便利です。

### 2-3. PowerShell に渡すコマンドは全体を `"..."` で囲む

引数は空白で区切られるため、囲まないと複数の引数に分かれてしまい、PowerShell 側で結合し直されるときに引用の扱いが変わって、空白を含むパスで壊れます。

```
NG   -NoProfile -Command Get-Item -LiteralPath '$p'
OK   -NoProfile -Command "Get-Item -LiteralPath '$p'"
```

囲んだ中では `'...'`（単引用符）を使います。

### 2-4. 引数に `"` そのものは書けない

`"` は引数の区切りとして解釈されて消えるため、子プロセスに `"` を渡すことはできません。文字列の引用が必要な場面では `'...'` を使ってください。ほとんどの CLI は単引用符でも問題なく動きますが、単引用符を受け付けないアプリに `"` 付きの値を渡したい場合は、いったんバッチファイルや `.ps1` に逃がすのが確実です。

### 2-5. `|` と `$` と `@` は `^` でエスケープする

引数の中でこれらの文字そのものを書きたいときは `^` を前に置きます。PowerShell のワンライナーではほぼ必ず出てきます。

| 書き方 | 結果 | 使う場面 |
|---|---|---|
| `^\|` | `\|` | PowerShell のパイプ |
| `^$` | `$` | PowerShell の変数（`^$_` `^$i` など） |
| `^@` | `@` | PowerShell の配列 `^@(...)`、7-Zip のリストファイル `^@list.txt` |

`^` は上記のような特殊文字の前に置いたときだけエスケープとして働くので、`C:\Foo^Bar\app.exe` のようなパスはそのまま書けます。逆に、`[` `,` `;` `%` `>` `=` などは特殊文字ではないので**そのまま書けます**（ffmpeg の `-filter_complex` や ImageMagick の `-resize 1280x1280>` はエスケープ不要）。

### 2-6. 環境変数は展開されない

`%LOCALAPPDATA%` や `%USERPROFILE%` は展開されません。実際のパスを書いてください。VS Code のように既定でユーザーごとの場所へ入るアプリは、ここを間違えがちです。

```
NG   @code = %LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe
OK   @code = C:\Users\yourname\AppData\Local\Programs\Microsoft VS Code\Code.exe
```

### 2-7. `.bat` / `.cmd` / `.ps1` は直接起動できない

実行ファイル欄に指定できるのは `.exe` です。バッチやスクリプトを呼びたいときは、インタプリタ経由にします。

```
バッチを実行 | @cmd        | /c C:\Tools\scripts\convert.bat $p
PS1 を実行   | @powershell | -NoProfile -ExecutionPolicy Bypass -File C:\Tools\scripts\convert.ps1 -Path "$p"
```

`.bat` を直接書いた場合は、選んだ瞬間に「起動できませんでした」のダイアログが出ます（`--check` はパスが実在するかしか見ないので、ここは通ってしまいます）。

### 2-8. `+`（まとめて渡す）が向くもの・向かないもの

`+` を付けると、選択したすべてのパスが `$p` の位置にまとめて展開されます。

- **向く** — 複数の入力を並べて受け取れるアプリ。7-Zip の圧縮、ImageMagick の `+append`、VLC のプレイリスト、VS Code の `--diff`
- **向かない** — 入力ごとにオプションが必要なアプリ。ffmpeg の `-i $p` は `-i a.mkv b.mkv` に展開されてしまい、意図どおりになりません

ffmpeg で複数ファイルを扱いたい場合は、`+` を使わず（＝ファイルごとに 1 プロセス起動する既定の動作のまま）並列に走らせるか、PowerShell でラップしてループを書きます。

**`$p` は独立した 1 つの引数として書きます。** `-i$p` のようにつなげて書くと、展開されるのは最初の 1 つだけで、残りは黙って捨てられます。`--check` が警告するので、`+` を付けた項目を書いたら一度通しておくとよいです。

```
OK   + まとめて圧縮 | @7z     | a -t7z $d\archive.7z $p -mx9
NG   + 変換         | @ffmpeg | -i$p -c copy out.mkv
```

### 2-9. 出力ファイルの上書き事故を防ぐ

ExtRun からの一発実行は、確認ダイアログが出ないぶん事故も一瞬です。このドキュメントのレシピは次の方針で書いてあります。同じ方針で書き足すことをおすすめします。

- 出力先は `$-p_suffix.ext` のように**元と違う名前**にする（`$-p.mp4` のように拡張子で区別できる場合はそのまま）
- 上書き拒否のオプションがあるアプリでは必ず付ける（ffmpeg の `-n`、7-Zip の `-aos`）
- 元ファイルを消す・置き換えるレシピは作らない

---

## 3. ffmpeg — 動画・音声

> 想定バージョン: ffmpeg 6.x / 7.x（[gyan.dev](https://www.gyan.dev/ffmpeg/builds/) の full build など）
> GUI を持たないツールなので、ExtRun から呼べることの価値が一番はっきり出ます。

```
@ff       = C:\Tools\ffmpeg\bin
@ffmpeg   = @ff\ffmpeg.exe
```

### 3-1. まず情報を見る

```
[@動画 @音声]

情報を表示
 | @powershell
 | -NoProfile -NoExit -Command "& '@ff\ffprobe.exe' -v error -show_entries format=duration,size,bit_rate:stream=index,codec_type,codec_name,width,height,r_frame_rate -of default=noprint_wrappers=1 '$p'"
```

> **使用**: PowerShell ラップ / `-NoExit` / `'@ff\ffprobe.exe'`（[2-2](#2-2-別名は引用符では終わらない)）
> `:` や `,` や `=` はエスケープ不要でそのまま書けます。

### 3-2. 動画を変換する

```
[@動画]

変換
> MP4 (H.264) にする [-.mp4]
 | @ffmpeg
 | -n -i $p -c:v libx264 -crf 23 -preset medium -c:a aac -b:a 192k $-p.mp4
> MP4 (H.265) にする
 | @ffmpeg
 | -n -i $p -c:v libx265 -crf 28 -preset medium -c:a aac -b:a 192k $-p_h265.mp4
> ---
> 音声だけ取り出す（無劣化・m4a）
 | @ffmpeg
 | -n -i $p -vn -c:a copy $-p.m4a
> 音声だけ取り出す（MP3）
 | @ffmpeg
 | -n -i $p -vn -c:a libmp3lame -q:a 2 $-p.mp3
```

> **使用**: `[-.mp4]`（引き算）/ `$-p` / 階層メニュー / セパレーター
> `[-.mp4]` を付けると、元が MP4 のときだけ「MP4 にする」が消えます。H.265 のほうは出力名を `_h265` で分けているので、MP4 に対しても意味があり、引き算していません。
> `-c:a copy` は元の音声が AAC のときだけ成功します。失敗しても直接起動では見えないので、うまくいかないときは [2-1](#2-1-黒い窓が一瞬で消えて結果が見えない) のラップ版で確かめてください。

### 3-3. GIF にする

```
GIF にする（パレット最適化）
 | @ffmpeg
 | -n -i $p -filter_complex fps=12,scale=480:-1:flags=lanczos,split[a][b];[b]palettegen[p];[a][p]paletteuse -loop 0 $-p.gif
```

> **使用**: 特殊文字でない記号はそのまま書ける（[2-5](#2-5--と--と--は--でエスケープする)）
> `[` `]` `;` `,` `:` はどれもエスケープ不要です。フィルタ全体に空白が無いので、引用符で囲む必要もありません。**空白を入れると別々の引数に分かれて壊れる**点だけ注意してください。

### 3-4. サムネイル・切り出し

```
サムネイルを作る
> 先頭フレーム        | @ffmpeg | -n -i $p -frames:v 1 $-p_thumb.png
> 5 秒地点            | @ffmpeg | -n -ss 5 -i $p -frames:v 1 $-p_thumb.png
> 10 秒ごとに連番     | @ffmpeg | -n -i $p -vf fps=1/10 $d\$a_%03d.png

無劣化で切り出す
> 先頭 30 秒 | @ffmpeg | -n -t 30 -i $p -c copy $-p_cut.mp4
> 30 秒以降  | @ffmpeg | -n -ss 30 -i $p -c copy $-p_cut.mp4
```

> **使用**: `$d\$a_...` で出力先を組み立てる
> `$-p_%03d.png` でも同じですが、`$d`（親フォルダ）と `$a`（拡張子なしの名前）に分けて書くと、出力先だけ別フォルダに変えたくなったときに直しやすくなります。
> `-c copy` の切り出しはキーフレーム単位でしか切れないため、指定より少しずれます。正確に切りたい場合は再エンコード（`-c:v libx264`）にしてください。

### 3-5. 回転・反転

```
回転・反転
> 右 90 度 | @ffmpeg | -n -i $p -vf transpose=1 -c:a copy $-p_rot.mp4
> 左 90 度 | @ffmpeg | -n -i $p -vf transpose=2 -c:a copy $-p_rot.mp4
> 左右反転 | @ffmpeg | -n -i $p -vf hflip  -c:a copy $-p_flip.mp4
```

### 3-6. 音声

```
[@音声]

変換
> MP3 にする [-.mp3] | @ffmpeg | -n -i $p -c:a libmp3lame -q:a 2 $-p.mp3
> WAV にする [-.wav] | @ffmpeg | -n -i $p -c:a pcm_s16le $-p.wav
> FLAC にする [-.flac] | @ffmpeg | -n -i $p -c:a flac $-p.flac

音量をそろえる
 | @ffmpeg
 | -n -i $p -af loudnorm=I=-16:TP=-1.5:LRA=11 -c:a aac -b:a 192k $-p_norm.m4a

波形を画像にする
 | @ffmpeg
 | -n -i $p -filter_complex showwavespic=s=1200x300 -frames:v 1 $-p_wave.png
```

---

## 4. ImageMagick / IrfanView — 画像

> 想定バージョン: ImageMagick 7.x（`magick.exe` の 1 本にまとまった世代）/ IrfanView 4.6x + 64bit 版
> 同じ用途を CLI 派（ImageMagick）と GUI 派（IrfanView）の両方で書いてあります。両方使う必要はありません。

```
@magick = C:\Program Files\ImageMagick\magick.exe
@irfan  = C:\Program Files\IrfanView\i_view64.exe
```

### 4-1. ImageMagick — 形式変換

```
[@画像]

形式を変換
> PNG にする  [-.png]        | @magick | $p $-p.png
> JPEG にする [-.jpg -.jpeg] | @magick | $p -quality 90 $-p.jpg
> WebP にする [-.webp]       | @magick | $p -quality 85 $-p.webp
> ---
> ICO にする（複数サイズ入り）
 | @magick
 | $p -define icon:auto-resize=256,128,64,48,32,16 $-p.ico
```

> **使用**: `[-.png]` `[-.jpg -.jpeg]`（引き算は複数書ける）
> 「JPEG にする」は `.jpg` と `.jpeg` の両方を引かないと、`.jpeg` のファイルに対して自分自身への変換が出てしまいます。

### 4-2. ImageMagick — サイズと加工

```
サイズを変える
> 長辺 1280px に縮小 | @magick | $p -resize 1280x1280> $-p_1280.png
> 幅 800px にそろえる | @magick | $p -resize 800x      $-p_800.png
> 50% に縮小          | @magick | $p -resize 50%       $-p_half.png

加工
> グレースケール   | @magick | $p -colorspace Gray $-p_gray.png
> 余白を切り取る   | @magick | $p -trim +repage   $-p_trim.png
> Exif を取り除く  | @magick | $p -strip          $-p_clean.jpg
```

> **使用**: 記号をそのまま書く
> `-resize 1280x1280>` の `>`（「大きいときだけ縮小」の意味）は、行頭ではないのでサブメニューのマーカーとは解釈されません。`+repage` の `+` も同様です。

### 4-3. ImageMagick — 複数選択をまとめる

```
+ 横に並べて 1 枚にする | @magick | $p +append $d\montage.png
+ 縦に並べて 1 枚にする | @magick | $p -append $d\montage.png
+ コンタクトシートを作る
 | @magick
 | montage $p -tile 4x -geometry +4+4 $d\contact.png
```

> **使用**: `+`（まとめて 1 プロセスに渡す）
> `$p` の位置に選択したすべてのパスが展開されるので、`magick a.png b.png c.png +append out.png` という形になります。`+` の一番分かりやすい使い道です。

### 4-4. ImageMagick — 情報を見る

```
画像情報を表示
 | @powershell
 | -NoProfile -NoExit -Command "& 'C:\Program Files\ImageMagick\magick.exe' identify -format '%f : %wx%h  %m  %[colorspace]\n' '$p'"
```

> ここでは実行ファイルを別名にせず素のパスで書いています。`'@magick'` と書くと `magick'` という別名を探しに行くためです（[2-2](#2-2-別名は引用符では終わらない)）。別名を使いたい場合は `@apps\ImageMagick\magick.exe` のように `\` を挟む形にしてください。

### 4-5. IrfanView

```
[@画像]

IrfanView で開く | @irfan | $p

IrfanView で変換
> JPEG にする [-.jpg -.jpeg] | @irfan | $p /convert=$-p.jpg
> PNG にする  [-.png]        | @irfan | $p /convert=$-p.png
> 長辺 1280px に縮小
 | @irfan
 | $p /resize_long=1280 /aspectratio /resample /convert=$-p_1280.jpg


[folder]

IrfanView でサムネイル表示 | @irfan | /thumbs $p
```

> **使用**: 型 1（渡すだけ）と型 2（CLI）の中間
> IrfanView は GUI アプリでありながらコマンドラインオプションが豊富で、ExtRun とは相性の良い部類です。オプションについては IrfanView のインストールフォルダにある `i_options.txt` を参考にして下さい。使えるオプションはバージョンで増減するので、初回は必ず 1 枚で試してください。

---

## 5. 7-Zip — 圧縮・展開

> 想定バージョン: 7-Zip 23.x / 24.x
> ExtRun から利用する用途としては需要が一番大きい分野です。同梱サンプルの `tar.exe` の項目を、そのまま置き換えられます。

```
@7z  = C:\Program Files\7-Zip\7z.exe
@7zg = C:\Program Files\7-Zip\7zG.exe
```

`7z.exe` はコンソール版、`7zG.exe` は**進捗ダイアログが出る GUI 版**です。時間のかかる操作は `7zG.exe`、結果の文字列を読みたい操作は `7z.exe` を PowerShell でラップ、と使い分けると快適です。

### 5-1. 展開

```
[@書庫]

展開
> 同じ名前のフォルダに展開 | @7zg | x -o$-p -y $p
> このフォルダに展開       | @7zg | x -o$d  -y $p
> 上書きせずに展開         | @7zg | x -o$-p -aos $p
```

> **使用**: `$-p`（拡張子なしフルパス）/ `$d`（親フォルダ）
> **`-o` と展開先の間に空白を入れられない**のが 7-Zip の特徴です。`-o$-p` は展開すると `-oC:\work\archive` のような 1 つの引数になります。パスに空白が含まれていても、ExtRun は引数を 1 つのまとまりとして子プロセスに渡すので問題ありません。
> `.tar.gz` のような二重拡張子は `$-p` で `.gz` しか落ちない点に注意してください（`archive.tar.gz` → `archive.tar`）。

### 5-2. 中身を確認する

```
中身を一覧する
 | @powershell
 | -NoProfile -NoExit -Command "& 'C:\Program Files\7-Zip\7z.exe' l '$p'"

書庫をテストする（壊れていないか）
 | @powershell
 | -NoProfile -NoExit -Command "& 'C:\Program Files\7-Zip\7z.exe' t '$p'"
```

### 5-3. 圧縮

```
[file folder]

圧縮
> ZIP
>> 個別に圧縮        | @7zg | a -tzip $-p.zip $p
>> + まとめて 1 つに | @7zg | a -tzip $d\$f.zip $p
> 7Z
>> 個別に圧縮        | @7zg | a -t7z $-p.7z $p
>> 個別に圧縮（最高圧縮） | @7zg | a -t7z -mx9 $-p.7z $p
>> + まとめて 1 つに | @7zg | a -t7z -mx9 $d\$f.7z $p
> ---
> 分割して圧縮（100MB ごと） | @7zg | a -t7z -v100m $-p.7z $p
> パスワード付き 7z（ファイル名も隠す）
 | @powershell
 | -NoProfile -NoExit -Command "& 'C:\Program Files\7-Zip\7z.exe' a -t7z -mhe=on -p '$-p.7z' '$p'"
```

> **使用**: 2 階層のサブメニュー（`>` `>>`）/ `+` / `$f`（親フォルダ名）
> `$d\$f.zip` は「選択したものが入っているフォルダと同じ名前の zip を、その隣に作る」という意味になります（`C:\work\a.txt` を選ぶと `C:\work\work.zip`）。まとめて圧縮するときの命名として自然です。
> パスワードは対話入力が必要なので、値を書かない `-p` を使い、PowerShell 経由で入力欄を出しています。**設定ファイルにパスワードを直接書かないでください**（`--check` の出力にも載ります）。

---

## 6. VS Code — 開く・比べる

> 既定ではユーザーごとの場所にインストールされます。System Installer を使った場合は `C:\Program Files\Microsoft VS Code\Code.exe` です。

```
@code = C:\Users\yourname\AppData\Local\Programs\Microsoft VS Code\Code.exe
```

```
[file folder]

VS Code で開く                   | @code | $p
+ まとめて新しいウィンドウで開く | @code | -n $p


[folder]

このフォルダをワークスペースに追加 | @code | -a $p


[file]

+ 2 つのファイルを比較 | @code | --diff $p
```

> **使用**: `+` の実用例
> `--diff` はちょうど 2 つのファイルを受け取るオプションなので、`+ 2 つのファイルを比較` はエクスプローラで 2 つ選んで実行する前提の項目になります。3 つ以上選ぶと VS Code 側でエラーになります。
> GUI の差分ツールを使うなら WinMerge も同じ書き方です。こちらはフォルダ同士の比較もできます。
>
> ```
> @winmerge = C:\Program Files\WinMerge\WinMergeU.exe
>
> [file folder]
> + WinMerge で比較 | @winmerge | -e -r $p
> ```

---

## 7. VLC — 再生する

```
@vlc = C:\Program Files\VideoLAN\VLC\vlc.exe
```

```
[@動画 @音声]

VLC で再生                     | @vlc | $p
+ 選んだ順にまとめて再生       | @vlc | $p
+ シャッフルして再生           | @vlc | --random $p


[folder]

フォルダごと再生 | @vlc | $p
```

> **使用**: `+`
> VLC は複数のパスを引数で受け取ると**そのままプレイリストになります**。「選んだファイルだけを順に流したい」という用途は意外と多く、`+` の効果が一番わかりやすい例です。
> 「VLC で再生」自体は関連付けと重複しますが、ExtRun に置く意味は**関連付けを変えずに複数のプレイヤーを並べられる**ことにあります。MPC-BE や PotPlayer と並べて、用途で使い分けてください。

---

## 8. ターミナル / WSL / Git — フォルダで作業を始める

フォルダを対象にした「ここで開く」系は、需要のわりに書き方の情報が少ない分野です。`:dir`（作業フォルダ）の出番でもあります。

```
@sys = C:\Windows\System32
@cmd = @sys\cmd.exe
@powershell = @sys\WindowsPowerShell\v1.0\powershell.exe
@pwsh = C:\Program Files\PowerShell\7\pwsh.exe
@wt   = C:\Users\yourname\AppData\Local\Microsoft\WindowsApps\wt.exe
@git  = C:\Program Files\Git
```

```
[folder]

ここで開く
> Windows Terminal     | @wt   | -d $p
> PowerShell 7         | @pwsh | -NoExit -Command "Set-Location -LiteralPath '$p'"
> Windows PowerShell   | @powershell | -NoExit -Command "Set-Location -LiteralPath '$p'"
> コマンドプロンプト   | @cmd  |
 :dir $p
> ---
> WSL       | @sys\wsl.exe        | --cd $p
> Git Bash  | @git\git-bash.exe   | --cd=$p
```

> **使用**: `:dir`（作業フォルダ）/ 引数を空にする書き方
> `| @cmd |` のように**行末を `|` で終える**と「引数なしで起動」になります（省略した場合の `$p` とは区別されます）。そのうえで `:dir $p` を付けると、そのフォルダをカレントディレクトリにして起動します。
> `wt.exe` は WindowsApps 内の実行エイリアスです。`--check` で見つからない場合は、`(Get-Command wt).Source` で実体を確認してください。

```
Git
> 状態を見る (git status)
 | @powershell
 | -NoProfile -NoExit -Command "& '@git\cmd\git.exe' status"
 :dir $p
> 履歴を見る (git log)
 | @powershell
 | -NoProfile -NoExit -Command "& '@git\cmd\git.exe' log --oneline --graph --decorate -30"
 :dir $p
```

> `:dir $p` でそのフォルダに移ってから git を呼ぶので、コマンド側にパスを渡す必要がありません。`:dir` を使わない場合は `git -C '$p' status` と書いても同じです。

---

## 9. Pandoc — 文書を変換する

> 引数が短く、拡張子の指定（継承・引き算・完全置換）の効きが分かりやすいので、書式を覚えるのにも向いています。

```
@pandoc = C:\Tools\pandoc\pandoc.exe
```

```
[.md .markdown]

変換
> HTML にする（単体ファイル） | @pandoc | $p -s --embed-resources -o $-p.html
> Word にする                 | @pandoc | $p -o $-p.docx
> PDF にする
 | @pandoc
 | $p -o $-p.pdf --pdf-engine=xelatex -V CJKmainfont=Meiryo


[.docx .odt .html]

Markdown にする | @pandoc | $p -t gfm -o $-p.md
```

> **使用**: セクション見出しに複数拡張子 / `$-p`
> PDF 出力には別途 TeX 環境（MiKTeX など）が必要です。日本語を含む場合は `-V CJKmainfont` でフォントの指定が要ります。

---

## 10. 画像最適化 — oxipng / pngquant / cwebp

Web 用の画像を扱う人向け。どれも単機能の CLI なので、レシピの型 2 の練習にちょうど良い題材です。

```
@tools    = C:\Tools
@oxipng   = @tools\oxipng\oxipng.exe
@pngquant = @tools\pngquant\pngquant.exe
@cwebp    = @tools\libwebp\bin\cwebp.exe
```

```
[.png]

最適化
> 可逆圧縮（oxipng） | @oxipng   | -o max --strip safe --out $-p_opt.png $p
> 減色（pngquant）   | @pngquant | --quality=65-90 --output $-p_q.png -- $p


[@画像]

WebP に変換 [-.webp]
> 標準     | @cwebp | $p -preset default -sharp_yuv -af -mt -o $-p.webp
> 可逆圧縮 | @cwebp | $p -lossless -af -mt -o $-p_lossless.webp
```

> **使用**: `[-.webp]`（引き算）
> `pngquant` の `--` は「これ以降はオプションではない」という区切りで、`-` で始まる名前のファイルでも安全に扱えます。ExtRun 側の記号ではないので、そのまま書けます。

### おまけ — フォルダ内の画像だけまとめる

```
[folder]

画像だけ ZIP にまとめる
 | @powershell
 | -NoProfile -NoExit -Command "^$ext=^@('.png','.jpg','.jpeg','.gif','.webp'); ^$list=Get-ChildItem -LiteralPath '$p' -Recurse -File ^| Where-Object { ^$ext -contains ^$_.Extension }; & 'C:\Program Files\7-Zip\7z.exe' a -tzip '$p-images.zip' ^$list.FullName"
```

> **使用**: `^$`（PowerShell の変数）/ `^@`（PowerShell の配列）/ `^|`（パイプ）
> ExtRun のプレースホルダー `$p` と PowerShell の変数 `^$ext` が同じ引数の中に共存しています。`^` を付け忘れると、`$e` がプレースホルダー（拡張子）として食われて壊れます。

---

## 付録 A. 書式の逆引き

「この書き方の実例が見たい」ときの索引です。

| 書式 | 意味 | 実例のあるレシピ |
|---|---|---|
| `$p` | フルパス | 全編 |
| `$-p` | 拡張子なしフルパス | [3-2](#3-2-動画を変換する) / [5-1](#5-1-展開) |
| `$d` | 親フォルダのパス | [3-4](#3-4-サムネイル切り出し) / [5-1](#5-1-展開) |
| `$a` | 拡張子なしの名前 | [3-4](#3-4-サムネイル切り出し)（連番サムネイル） |
| `$f` | 親フォルダの名前 | [5-3](#5-3-圧縮)（まとめて 1 つに） |
| `[-.mp4]` | 継承した拡張子から引く | [3-2](#3-2-動画を変換する) / [4-1](#4-1-imagemagick--形式変換) |
| `[.svg]` | 継承を無視して置き換える | [4-5](#4-5-irfanview)（フォルダ用の項目） |
| `+` | 全パスを 1 プロセスに渡す | [4-3](#4-3-imagemagick--複数選択をまとめる) / [5-3](#5-3-圧縮) / [6](#6-vs-code--開く比べる) / [7](#7-vlc--再生する) |
| `:dir` | 作業フォルダ | [8](#8-ターミナル--wsl--git--フォルダで作業を始める) |
| `>` `>>` | サブメニュー | [3-2](#3-2-動画を変換する) / [5-3](#5-3-圧縮) |
| `^\|` `^$` `^@` | エスケープ | [10](#おまけ--フォルダ内の画像だけまとめる) |
| 行末を `\|` で終える | 引数なしで起動 | [8](#8-ターミナル--wsl--git--フォルダで作業を始める)（コマンドプロンプト） |
| 別名の中の別名 | `@ff = @tools\ffmpeg\bin` | [1-3](#1-3-別名で直す場所を-1-か所にまとめる) |

---

## 付録 B. 別名まとめ（コピペ用）

このドキュメントで使った別名の一覧です。設定ファイルの先頭に貼り、**使うものだけ残して、パスを自分の環境に直して**ください。

```
# --- 場所 ---
@apps  = C:\Program Files
@tools = C:\Tools
@sys   = C:\Windows\System32
@local = C:\Users\yourname\AppData\Local

# --- Windows 標準 ---
@cmd        = @sys\cmd.exe
@powershell = @sys\WindowsPowerShell\v1.0\powershell.exe
@explorer   = C:\Windows\explorer.exe

# --- 動画・音声 ---
@ff     = @tools\ffmpeg\bin
@ffmpeg = @ff\ffmpeg.exe
@vlc    = @apps\VideoLAN\VLC\vlc.exe

# --- 画像 ---
@magick   = @apps\ImageMagick\magick.exe
@irfan    = @apps\IrfanView\i_view64.exe
@oxipng   = @tools\oxipng\oxipng.exe
@pngquant = @tools\pngquant\pngquant.exe
@cwebp    = @tools\libwebp\bin\cwebp.exe

# --- 書庫 ---
@7z  = @apps\7-Zip\7z.exe
@7zg = @apps\7-Zip\7zG.exe

# --- 開発・文書 ---
@code     = @local\Programs\Microsoft VS Code\Code.exe
@winmerge = @apps\WinMerge\WinMergeU.exe
@pwsh     = @apps\PowerShell\7\pwsh.exe
@wt       = @local\Microsoft\WindowsApps\wt.exe
@git      = @apps\Git
@pandoc   = @tools\pandoc\pandoc.exe

# --- 拡張子 ---
@画像     = .png .jpg .jpeg .gif .bmp .tif .tiff .webp .avif
@動画     = .mp4 .mkv .avi .wmv .mov .flv .webm .ts .m2ts
@音声     = .wav .mp3 .m4a .aac .flac .wma .opus .ogg
@書庫     = .zip .7z .rar .tar .gz .tgz .bz2 .xz .lzh .cab .iso
@テキスト = .txt .md .log .csv .ini .json .xml .yaml .yml
```

書き足したら、忘れずに確認してください。

```powershell
extrun.exe --check
```

---

## 付録 C. AutoHotkey から呼び出す

ExtRun は「引数で渡されたパスに対してメニューを出す」だけのプログラムなので、**呼び出し方は自由**です。エクスプローラで選択中のファイルを AutoHotkey でホットキーに割り当てれば、右クリックすら経由せずにメニューを開けます。

### C-1. 基本のスクリプト（AutoHotkey v2）

```ahk
#Requires AutoHotkey v2.0
#SingleInstance Force

; extrun.exe の場所（自分の環境に合わせて変更）
ExtRunPath := "C:\Tools\ExtRun\extrun.exe"

; エクスプローラのウィンドウでだけ Ctrl+Alt+X を有効にする（任意のホットキー可）
#HotIf WinActive("ahk_class CabinetWClass") || WinActive("ahk_class ExploreWClass")
^!x:: SendToExtRun()
#HotIf

SendToExtRun() {
    paths := GetExplorerSelection()
    if !paths.Length {
        Notify("対象が見つかりません")
        return
    }

    args := ""
    for path in paths
        args .= ' "' path '"'

    try
        Run '"' ExtRunPath '"' args
    catch as err
        Notify("起動できませんでした: " err.Message)
}

; アクティブなエクスプローラで選択中の項目のフルパスを配列で返す。
; 何も選択されていなければ、表示中のフォルダ自身を返す。
GetExplorerSelection() {
    paths := []
    hwnd := WinExist("A")

    for window in ComObject("Shell.Application").Windows {
        ; 一覧には hwnd を持たない項目が混ざるので、1 件ずつ try で守る。
        ; catch を書かずに次の行へ落とすと、そこで break してしまう。
        try {
            if window.hwnd != hwnd
                continue
            doc := window.Document
            for item in doc.SelectedItems
                paths.Push(item.Path)
            if !paths.Length
                paths.Push(doc.Folder.Self.Path)
        } catch {
            continue
        }
        break
    }
    return paths
}

Notify(text) {
    ToolTip text
    SetTimer () => ToolTip(), -1500
}
```

**動作**

1. アクティブウィンドウのハンドルと一致するエクスプローラを `Shell.Application` の一覧から探す
2. その `SelectedItems` からフルパスを集める
3. 各パスを `"` で囲んで `extrun.exe` に渡す

ExtRun 側の設定は何も要りません。渡されたパスの種類（拡張子・フォルダ）に応じて、いつもどおりメニューが出ます。

**ファイルを選ばずにフォルダを対象にできる**のがこの書き方の地味に便利なところです。何も選択していない状態でホットキーを押すと、表示中のフォルダが対象になり、`[folder]` セクションの項目（「ここでターミナルを開く」など）がそのまま使えます。

> [!IMPORTANT]
> **`Shell.Application` の一覧には、ウィンドウではない項目が混ざります。**
>
> Windows 11 では、`Windows` コレクションの先頭に `hwnd` プロパティを持たない項目が入っていることがあります。AutoHotkey v2 でこれに触ると `PropertyError: This value of type "ComObject" has no property named "hwnd"` が発生します。
>
> 厄介なのは、`catch` のない `try` がこのエラーを**握りつぶして次の行へ進む**ことです。ループの外側に `break` を置いていると、1 件目でループが終わってしまい、いつまでも「対象が見つかりません」になります。エラーダイアログも出ないので原因が分かりません。
>
> 上のコードのように `catch { continue }` を書いて、**エラーが起きた項目は飛ばして次を見る**ようにしてください。

### C-2. デスクトップにも対応させる

デスクトップの選択項目は別の方法で取得します。必要な場合だけ足してください。

```ahk
#HotIf WinActive("ahk_class Progman") || WinActive("ahk_class WorkerW")
^!x:: SendToExtRun(true)
#HotIf

; SendToExtRun / GetExplorerSelection に引数を足す
SendToExtRun(isDesktop := false) {
    paths := isDesktop ? GetDesktopSelection() : GetExplorerSelection()
    ; …以降は C-1 と同じ…
}

GetDesktopSelection() {
    paths := []
    ; 8 = SWC_DESKTOP, 1 = SWFO_NEEDDISPATCH
    desktop := ComObject("Shell.Application").Windows.FindWindowSW(0, 0, 8, 0, 1)
    for item in desktop.Document.SelectedItems
        paths.Push(item.Path)
    return paths
}
```

Windows のバージョンやデスクトップ拡張ツールの有無で `Progman` / `WorkerW` のどちらが前面に来るかが変わるため、両方を条件に入れています。

### C-3. AutoHotkey v1.1 の場合

```ahk
#SingleInstance, Force
ExtRunPath := "C:\Tools\ExtRun\extrun.exe"

#IfWinActive ahk_class CabinetWClass
^!x::SendToExtRun()
#IfWinActive

SendToExtRun() {
    global ExtRunPath
    args := ""
    hwnd := WinExist("A")
    for window in ComObjCreate("Shell.Application").Windows {
        try {
            if (window.hwnd != hwnd)
                continue
            doc := window.Document
            for item in doc.SelectedItems
                args .= " """ item.Path """"
            if (args = "")
                args := " """ doc.Folder.Self.Path """"
        } catch e {
            continue
        }
        break
    }
    if (args = "") {
        ToolTip, 対象が見つかりません
        SetTimer, RemoveToolTip, -1500
        return
    }
    Run, "%ExtRunPath%"%args%
}

RemoveToolTip:
    ToolTip
return
```

### C-4. 実用上の注意

**選択項目がすべて実在するパスとは限らない**

zip の中身、検索結果、ライブラリ、「クイックアクセス」などでは、`item.Path` が `::{20D04FE0-...}` のような仮想パスや、書庫内の擬似パスになることがあります。ExtRun 側では「見つかりません」のエラーになるので、気になる場合は集める前に弾いてください。

```ahk
        for item in window.Document.SelectedItems
            if FileExist(item.Path)
                paths.Push(item.Path)
```

**大量選択には上限がある**

Windows のコマンドラインは約 32,000 文字までです。数百個のファイルを一度に渡すと途中で切れるか、起動に失敗します。多数を扱うレシピは、親フォルダを対象にして設定側で `-Recurse` するほうが確実です。
