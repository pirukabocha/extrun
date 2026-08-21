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
///
/// **挿入の一覧（`INSERTS`）とは別の表**。こちらは「書かれているものを読む」
/// ための前方一致の見出しで、`$t{yyyyMMdd}` も `$t{HHmmss}` も同じ「日時」に
/// なる。中身まで並べると表が際限なく増える。
///
/// **並びが意味を持つ。** 長いものを先に置かないと、`$-p` の `-p` を拾い損ねて
/// 「フルパス」と読み違える。`$?int` などを `$?{` より先に置くのも同じ理由。
pub const MEANINGS: &[(&str, &str)] = &[
    ("$-p", "拡張子なしフルパス"),
    ("$?list", "一覧から選ぶ"),
    ("$?int", "整数の入力欄"),
    ("$?num", "数値の入力欄"),
    ("$?name", "ファイル名の入力欄"),
    ("$?{", "入力欄"),
    // **`$t` は `$t{` のときだけ日時**。単独の `$t` は素通しされる仕様なので、
    // ここでも中括弧まで含めて見る
    ("$t{", "日時"),
    ("$p", "フルパス"),
    ("$d", "親フォルダ"),
    ("$n", "ファイル名"),
    ("$a", "拡張子なしファイル名"),
    ("$f", "親フォルダ名"),
    ("$e", "拡張子"),
    ("$i", "何番目か"),
    ("$c", "総数"),
];

/// 「挿入」の一覧に並べるもの
///
/// **そのまま挿して意味のある形だけを置く。** かつては `$t` と `$?` を単独で
/// 並べていたが、どちらも挿しても効かない（`$t` は素通し、`$?` は入力欄に
/// ならない）。日時も入力欄も中括弧まで込みで 1 つの書き方なので、
/// **よく使う形を具体的に並べる**方が、書き方を覚えていない人には早い。
///
/// 3 つ目は、挿したあとに選択しておく部分。`$?{説明}` を挿したら `説明` が
/// 選ばれた状態になるので、そのまま打ち替えられる。空なら選択しない。
pub const INSERTS: &[(&str, &str, &str)] = &[
    ("$p", "フルパス", ""),
    ("$-p", "拡張子なしフルパス", ""),
    ("$d", "親フォルダ", ""),
    ("$n", "ファイル名", ""),
    ("$a", "拡張子なしファイル名", ""),
    ("$f", "親フォルダ名", ""),
    ("$e", "拡張子", ""),
    ("$i", "何番目か", ""),
    ("$i{000}", "何番目か（3 桁のゼロ埋め）", ""),
    ("$c", "総数", ""),
    ("$t{yyyyMMdd}", "日付（20250131）", ""),
    ("$t{HHmmss}", "時刻（143025）", ""),
    ("$t{yyyy-MM-dd}", "日付（区切りつき）", ""),
    ("$t{yyyyMMdd_HHmmss}", "日付と時刻", ""),
    ("$?{説明}", "入力欄", "説明"),
    ("$?{説明=既定値}", "入力欄（既定値つき）", "説明=既定値"),
    ("$?int{説明}", "入力欄（整数だけ）", "説明"),
    ("$?num{説明}", "入力欄（数値だけ）", "説明"),
    (
        "$?name{説明}",
        "入力欄（ファイル名に使える文字だけ）",
        "説明",
    ),
    (
        "$?list{説明=あ,い,う}",
        "入力欄（一覧から選ぶ）",
        "説明=あ,い,う",
    ),
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
        match MEANINGS.iter().find(|(mark, _)| rest.starts_with(mark)) {
            Some((mark, meaning)) => {
                // 見出しの `{` は表示に含めない（`$t{ — 日時` では読みにくい）
                let shown = mark.trim_end_matches('{');
                let line = format!("{} — {}", shown, meaning);
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

    #[test]
    fn 日時は中括弧まで見て拾う() {
        assert_eq!(used_placeholders("$t{yyyyMMdd}"), ["$t — 日時"]);
        // 単独の `$t` は素通しされる仕様なので、意味も出さない
        assert!(used_placeholders("$t だけ").is_empty());
    }

    #[test]
    fn 入力欄は決まりごとに読み分ける() {
        assert_eq!(used_placeholders("$?{幅}"), ["$? — 入力欄"]);
        assert_eq!(used_placeholders("$?int{幅}"), ["$?int — 整数の入力欄"]);
        assert_eq!(
            used_placeholders("$?list{向き=縦,横}"),
            ["$?list — 一覧から選ぶ"]
        );
    }

    /// 挿しても効かない形を一覧に置かない（かつて `$t` と `$?` を置いていた）
    #[test]
    fn 挿入の一覧はそのまま使える形だけ() {
        for (snippet, _, _) in INSERTS {
            assert!(
                !used_placeholders(snippet).is_empty(),
                "挿しても意味を持たない: {}",
                snippet
            );
        }
    }

    /// 選択しておく部分は、挿す文字列の中に必ずある
    #[test]
    fn 選択する部分は挿す文字列の中にある() {
        for (snippet, _, part) in INSERTS {
            if !part.is_empty() {
                assert!(snippet.contains(part), "{} に {} が無い", snippet, part);
            }
        }
    }
}
