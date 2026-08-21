/*!
「対象の種類」のひな型

**`extrun-make` 自身が持っている固定の一覧**で、ユーザーの設定ファイルとは
関係がない。設定ファイルがまだ無い人・別名を 1 つも書いていない人でも、
初回起動でそのまま使えることがこの表の役目。

中身は同梱サンプル（`extrun-config.txt`）と `docs/extrun-recipes.md` で実際に
使っている拡張子に合わせてある。**ここを増やすときは、そちらでも動く形か**を
確かめること（ひな型で選んだ拡張子は、そのまま `[...]` に書き出される）。

設定ファイルから読んだ別名（`@画像` など）は Phase 5 でこの上の層に足す。
そのとき、ひな型と同じ名前の別名が並ぶことがあるが、**どちらが正しいかは
決めない**。値まで見せて選ばせる。
*/

/// ひな型 1 つぶん
pub struct Preset {
    /// コンボに出す名前
    pub label: &'static str,
    /// 選んだときに拡張子の欄へ入る文字列
    pub extensions: &'static str,
}

/// 「自分で指定」（コンボのいちばん上）
///
/// 選ぶと拡張子の欄へフォーカスが移り、中身が選択状態になる。
/// 書く回数がいちばん多いので上に置いてある。
pub const CUSTOM: &str = "自分で指定";

/// ひな型の一覧
///
/// 並びは「よく書くもの」順ではなく、種類として並べたときに読みやすい順。
pub const PRESETS: &[Preset] = &[
    Preset {
        label: "画像",
        extensions: ".png .jpg .jpeg .gif .bmp .webp .avif",
    },
    Preset {
        label: "テキスト",
        extensions: ".txt .md .log .csv .ini .json .xml",
    },
    Preset {
        label: "動画",
        extensions: ".mp4 .mkv .avi .mov .wmv .webm",
    },
    Preset {
        label: "音声",
        extensions: ".mp3 .flac .wav .m4a .aac .opus",
    },
    Preset {
        label: "書庫",
        extensions: ".zip .7z .rar .tar .gz",
    },
    Preset {
        // ドットが要らないのはこの 2 つだけ
        label: "すべてのファイル",
        extensions: "file",
    },
    Preset {
        label: "フォルダ",
        extensions: "folder",
    },
];

/// 拡張子の並びに一致するひな型を探す
///
/// 欄を直に書き替えたときに「自分で指定」へ戻すための判定。空白の詰まり方だけが
/// 違う場合も同じものとみなす（`.png  .jpg` と `.png .jpg` を別扱いしない）。
pub fn find(extensions: &str) -> Option<usize> {
    let target = normalize(extensions);
    PRESETS
        .iter()
        .position(|preset| normalize(preset.extensions) == target)
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 引数の欄に書かれているプレースホルダーの意味
///
/// **早見表を常設しない**ための表。いま書かれているものだけを左から順に並べる
/// ので、`-a -c -f $d\images.zip $p` という字面が
/// 「親フォルダの images.zip に、フルパスを詰める」と読めるようになる。
/// 打ち間違えると何も出ないので、そこでも気づける。
pub const PLACEHOLDERS: &[(&str, &str)] = &[
    // **`$-p` を `$p` より先に見る**。逆にすると `$-p` の `-p` を拾い損ねる
    ("$-p", "拡張子なしフルパス"),
    ("$p", "フルパス"),
    ("$d", "親フォルダ"),
    ("$n", "ファイル名"),
    ("$a", "拡張子なしファイル名"),
    ("$f", "親フォルダ名"),
    ("$e", "拡張子"),
    ("$i", "何番目か"),
    ("$c", "総数"),
    ("$t", "日時"),
    ("$?", "入力欄"),
];

/// 引数の中に出てくるプレースホルダーを、書かれた順に拾う
///
/// エスケープ（`^$`）は数えない。**判定を自前で書かない**ために、
/// `$` の直前が `^` かどうかだけを見る（`text::escape_len` と同じ考え方）。
pub fn used_placeholders(args: &str) -> Vec<String> {
    let bytes = args.as_bytes();
    let mut found: Vec<String> = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        // `^$` は素の `$`
        if index > 0 && bytes[index - 1] == b'^' {
            index += 1;
            continue;
        }

        let rest = &args[index..];
        match PLACEHOLDERS.iter().find(|(mark, _)| rest.starts_with(mark)) {
            Some((mark, meaning)) => {
                let line = format!("{} — {}", mark, meaning);
                if !found.contains(&line) {
                    found.push(line);
                }
                index += mark.len();
            }
            None => index += 1,
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ひな型の拡張子から名前を引ける() {
        assert_eq!(
            find("file").map(|i| PRESETS[i].label),
            Some("すべてのファイル")
        );
        assert_eq!(
            find(".mp3 .flac .wav .m4a .aac .opus").map(|i| PRESETS[i].label),
            Some("音声")
        );
    }

    #[test]
    fn 空白の詰まり方は区別しない() {
        assert!(find("  file  ").is_some());
        assert!(find(".png   .jpg .jpeg .gif .bmp .webp .avif").is_some());
    }

    #[test]
    fn 当てはまらなければ自分で指定になる() {
        assert!(find(".xyz").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn 書かれたプレースホルダーだけを拾う() {
        let 拾った = used_placeholders(r"-a -c -f $d\images.zip $p");
        assert_eq!(拾った, ["$d — 親フォルダ", "$p — フルパス"]);
    }

    /// `$-p` を `$p` と読み違えると、意味の表示がずれる
    #[test]
    fn 拡張子なしフルパスを先に見る() {
        assert_eq!(used_placeholders("$-p.webp"), ["$-p — 拡張子なしフルパス"]);
    }

    #[test]
    fn エスケープしたものは数えない() {
        assert!(used_placeholders("^$path").is_empty());
    }

    #[test]
    fn 同じものは一度だけ並べる() {
        assert_eq!(used_placeholders("$p $p $p"), ["$p — フルパス"]);
    }

    #[test]
    fn 知らない書き方は何も出さない() {
        assert!(used_placeholders("$q $z").is_empty());
    }
}
