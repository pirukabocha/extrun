# 変更履歴

このファイルの書き方は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、
バージョン番号は [セマンティック バージョニング](https://semver.org/lang/ja/) に従います。

## [1.0.0] - 2026-08-14

初版公開。

### 機能

- 引数で渡されたファイル / フォルダの種類に応じて、カーソル位置にポップアップメニューを表示し、選ばれたコマンドを起動して終了する（常駐しない、ファイルも書き出さない）
- 1 行 1 項目のテキスト設定ファイル `extrun-config.txt`
  - セクション見出しによる拡張子の指定（`.txt` / `file` / `folder`）と、項目ごとの引き算 `[-.jpg]` / 完全置換 `[.svg]`
  - サブメニュー（`>` `>>`）、セパレーター（`---`）、まとめて実行（`+`）
  - 別名（`@名前 = 値`）、継続行（`|` で始まる行）、作業フォルダ（`:dir`）
  - プレースホルダー `$p` `$-p` `$d` `$n` `$a` `$f` `$e` と、`^` によるエスケープ
- `--check` による設定ファイルの検証（行番号付きの診断、エラーがあれば終了コード 1）
- `--version` / `--help`
- 起動に失敗したときのエラーダイアログ
- exe への VERSIONINFO の埋め込み

### ドキュメント

- `README.md` — 導入と概要
- `extrun-config-format.md` — 設定ファイル形式の完全仕様
- `extrun-recipes.md` — ffmpeg / ImageMagick / IrfanView / 7-Zip / VS Code / VLC / Pandoc などの設定例集と、AutoHotkey から呼び出す例
- `packaging/readme.txt` — 配布 zip に同梱する説明書

[1.0.0]: https://github.com/pirukabocha/extrun/releases/tag/v1.0.0
