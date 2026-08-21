/*!
フォームの中身と、そこから設定ファイルの数行を組み立てる部分

**このファイルがこのツールの中身**で、画面（`layout.rs` / `main.rs`）は
`Form` を埋めてここを呼ぶだけの入れ物にしてある。組み立てを画面の側に書くと
テストから触れなくなり、そのまま「動かしてみないと分からない」ものになる。

組み立てた文字列は、そのまま **`config::parse` に通せる**ことがテストで
確かめてある。エスケープの掛け方が正しいかどうかを字面で議論しても仕方が
ないので、**実際のパーサを通して名前が元に戻るか**で判定する。
*/

use extrun::text::{escape_name, escape_path};

/// 拡張子をどこに書くか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtStyle {
    /// セクションの見出しにする（`[.png .jpg]` の行が付く）
    Section,
    /// 項目の行に書く（`名前 [.png .jpg] | …`）
    ///
    /// 符号なし＝完全置換なので、**どのセクションの下に貼っても同じように動く**。
    PerItem,
}

/// メニューのどこに表示するか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// メニューの一番上の階層
    Root,
    /// 新しいサブメニューを作る（親の行も一緒に書き出す）
    NewSubmenu,
}

/// `:when` の値
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenKind {
    Always,
    Single,
    Multi,
}

impl WhenKind {
    fn keyword(self) -> Option<&'static str> {
        match self {
            WhenKind::Always => None,
            WhenKind::Single => Some("single"),
            WhenKind::Multi => Some("multi"),
        }
    }
}

/// 画面に入力されている内容
///
/// **文字列はどれも「人が打ったそのまま」**で、エスケープは `to_config` が
/// 掛ける。ここで掛けてしまうと、画面に出す値と設定ファイルに書く値が
/// 混ざって、どちらを表示しているのか分からなくなる。
#[derive(Debug, Clone)]
pub struct Form {
    // --- ① どんな項目にするか ---
    pub name: String,
    /// アクセスキー（半角英数字 1 文字。空なら付けない）
    pub key: String,
    pub app: String,
    pub args: String,
    /// 引数を渡さない（行末を `|` で終える）
    ///
    /// 引数欄を空にするのと欄ごと省くのは仕様で意味が違う（前者は引数なし、
    /// 後者は `$p` が渡る）ので、チェックで区別できるようにしてある。
    pub no_args: bool,
    /// 複数選んだら 1 回でまとめて渡す（`+`）
    pub all_mode: bool,

    // --- ② どのファイルで表示するか ---
    pub extensions: String,
    pub ext_style: ExtStyle,

    // --- ③ メニューのどこに表示するか ---
    pub placement: Placement,
    pub submenu_name: String,
    pub submenu_key: String,
    /// この項目の前に区切り線を入れる
    pub separator: bool,

    // --- 詳細設定 ---
    pub confirm: bool,
    pub confirm_message: String,
    pub admin: bool,
    pub wait: bool,
    pub delay: bool,
    pub delay_ms: String,
    pub when: WhenKind,
    pub dir: String,
    pub icon: String,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            name: String::new(),
            key: String::new(),
            app: String::new(),
            args: String::new(),
            no_args: false,
            all_mode: false,
            // 空のまま `[]` を書き出すと「対象の指定が空＝すべての対象」という
            // 意図しない意味になる。貼ってすぐ動く値から始める
            extensions: "file".to_string(),
            ext_style: ExtStyle::Section,
            placement: Placement::Root,
            submenu_name: String::new(),
            submenu_key: String::new(),
            separator: false,
            confirm: false,
            confirm_message: String::new(),
            admin: false,
            wait: false,
            delay: false,
            delay_ms: "300".to_string(),
            when: WhenKind::Always,
            dir: String::new(),
            icon: String::new(),
        }
    }
}

impl Form {
    /// 設定ファイルに貼り付ける数行を組み立てる
    pub fn to_config(&self) -> String {
        let mut out = String::new();

        // --- セクションの見出し ---
        //
        // 同じ見出しをファイル内に何度書いてもよいので、末尾に貼るぶんには
        // 毎回付けて構わない（仕様で決まっている）
        if self.ext_style == ExtStyle::Section {
            out.push('[');
            out.push_str(self.extensions.trim());
            out.push_str("]\r\n\r\n");
        }

        // --- サブメニューの親 ---
        if self.placement == Placement::NewSubmenu {
            out.push_str(&escape_name(self.submenu_name.trim()));
            out.push_str(&accesskey(&self.submenu_key));
            out.push_str("\r\n");
        }

        let marker = match self.placement {
            Placement::Root => "",
            Placement::NewSubmenu => "> ",
        };

        // --- 区切り線 ---
        //
        // 階層マーカーは項目と揃える。拡張子は**項目の行に書いているときだけ**
        // 付ける（同じセクションの中なら区切り線も同じものを継承するので、
        // 書かないと項目だけ消えて線が残る、ということが起きない）
        if self.separator {
            out.push_str(marker);
            out.push_str("---");
            if self.ext_style == ExtStyle::PerItem {
                out.push_str(&self.extension_suffix());
            }
            out.push_str("\r\n");
        }

        // --- 項目の行 ---
        out.push_str(marker);
        if self.all_mode {
            out.push_str("+ ");
        }
        out.push_str(&escape_name(self.name.trim()));
        out.push_str(&accesskey(&self.key));
        if self.ext_style == ExtStyle::PerItem {
            out.push_str(&self.extension_suffix());
        }

        if !self.app.trim().is_empty() {
            out.push_str(" | ");
            out.push_str(&escape_path(self.app.trim()));

            // 欄を空にする（行末を `|` で終える）のと、欄ごと省くのは意味が違う。
            // 前者は引数なし、後者は `$p` が渡る
            if self.no_args {
                out.push_str(" |");
            } else if !self.args.trim().is_empty() {
                out.push_str(" | ");
                out.push_str(self.args.trim());
            }
        }
        out.push_str("\r\n");

        // --- 名前付きフィールド ---
        //
        // 行頭に置く決まりなので、1 行 1 つ。並びは仕様書の表に合わせてある
        for line in self.named_fields() {
            out.push_str(" :");
            out.push_str(&line);
            out.push_str("\r\n");
        }

        out
    }

    /// `[.png .jpg]` の形（項目の行に書くとき）
    fn extension_suffix(&self) -> String {
        let extensions = self.extensions.trim();
        if extensions.is_empty() {
            return String::new();
        }
        format!(" [{}]", extensions)
    }

    /// 書き出す名前付きフィールドを順に返す
    fn named_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();

        if !self.dir.trim().is_empty() {
            fields.push(format!("dir {}", self.dir.trim()));
        }
        if self.confirm {
            let message = self.confirm_message.trim();
            if message.is_empty() {
                fields.push("confirm".to_string());
            } else {
                fields.push(format!("confirm {}", message));
            }
        }
        if !self.icon.trim().is_empty() {
            fields.push(format!("icon {}", self.icon.trim()));
        }
        if self.admin {
            fields.push("admin".to_string());
        }
        if self.delay {
            fields.push(format!("delay {}", self.delay_ms.trim()));
        }
        if self.wait {
            fields.push("wait".to_string());
        }
        if let Some(keyword) = self.when.keyword() {
            fields.push(format!("when {}", keyword));
        }

        fields
    }

    /// 貼り付け先の案内（④ の見出しに添える）
    ///
    /// **書き方で文言を変えない。** かつては「見出しにする」なら末尾、
    /// 「項目の行に書く」なら「どのセクションの下でも同じように動きます」と
    /// 出し分けていたが、どちらも言い過ぎだった。
    ///
    /// - 見出しを**ファイルの途中に貼ると、その下にある既存の項目の対象が
    ///   まとめて変わる**（見出しは「以降の項目」に効くため）。「どこでもよい」
    ///   と案内すると、黙って壊れる貼り方をさせてしまう
    /// - 項目の行に書いた場合も、サブメニューの途中に貼れば階層が切れる。
    ///   拡張子の指定が独立していることと、貼る位置が自由なことは別の話
    ///
    /// **末尾なら必ず正しい**ので、それだけを 1 つ言う。ほかの場所を禁じる
    /// 言い方にしないのは、整理して貼りたい人の邪魔をしないため。
    ///
    /// Phase 5 で「既にあるサブメニューの中」を選べるようにしたら、そのときは
    /// 貼り先の行番号を案内するのでここが再び変わる。
    pub fn paste_hint(&self) -> &'static str {
        "extrun-config.txt の末尾に貼り付ければ、そのまま動きます"
    }
}

/// ` (&X)` の形にする。空なら何も付けない
///
/// 日本語の名前には後ろに足す書き方が読みやすい、というのが仕様書の勧め。
/// 名前の中の文字に直接付ける書き方はツールでは扱わない（どの文字に付けるかを
/// 選ばせる UI が要るわりに、`(&O)` で困らない）。
fn accesskey(key: &str) -> String {
    let key = key.trim();
    match key.chars().next() {
        Some(ch) if ch.is_ascii_alphanumeric() => format!(" (&{})", ch),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use extrun::config;

    /// よくある形をひととおり埋めたフォーム
    fn 見本() -> Form {
        Form {
            name: "ZIP にまとめる".to_string(),
            key: "Z".to_string(),
            app: r"%SystemRoot%\System32\tar.exe".to_string(),
            args: r"-a -c -f $d\images.zip $p".to_string(),
            all_mode: true,
            extensions: ".png .jpg .jpeg".to_string(),
            ..Form::default()
        }
    }

    /// 組み立てた文字列を実際のパーサに通す
    ///
    /// **エスケープが正しいかを字面で議論しない。** パーサを通して元に戻るか
    /// どうかだけを見る。ここが通っていれば、`text::escape_name` の対象を
    /// 増やしても減らしても事故は起きない
    fn 通す(form: &Form) -> config::Parsed {
        config::parse(&form.to_config())
    }

    fn 診断(parsed: &config::Parsed) -> String {
        parsed
            .diags
            .iter()
            .map(|d| format!("{}行目 {}", d.line, d.message))
            .collect::<Vec<_>>()
            .join(" / ")
    }

    #[test]
    fn 見本はそのまま貼れる形になる() {
        let form = 見本();
        assert_eq!(
            form.to_config().replace("\r\n", "\n"),
            "[.png .jpg .jpeg]\n\
             \n\
             + ZIP にまとめる (&Z) | %SystemRoot%\\System32\\tar.exe | -a -c -f $d\\images.zip $p\n"
        );
    }

    #[test]
    fn 見本はパーサを通る() {
        let parsed = 通す(&見本());
        assert!(!parsed.has_error(), "{}", 診断(&parsed));
        assert_eq!(parsed.config.apps.len(), 1);
        // 表示名にはキーの丸括弧が残る（消えるのは `&` だけ）
        assert_eq!(parsed.config.apps[0].name, "ZIP にまとめる (Z)");
        assert!(parsed.config.apps[0].all_mode);
    }

    #[test]
    fn アクセスキーは名前の後ろに足す() {
        let parsed = 通す(&見本());
        let item = &parsed.config.apps[0];
        // 表示名にはキーの `&` も丸括弧の中身も残る（`&` だけが消える）
        assert_eq!(item.name, "ZIP にまとめる (Z)");
        assert!(item.accesskey.is_some());
    }

    #[test]
    fn キーが空なら付けない() {
        let form = Form {
            key: String::new(),
            ..見本()
        };
        assert!(!form.to_config().contains('&'));
    }

    /// 記号を含む名前が、パーサを通して元に戻ること
    #[test]
    fn 記号を含む名前が元に戻る() {
        let 名前 = [
            "Q&A を開く",
            "C++ で開く",
            "PNG - JPEG",
            "注意: 上書きする",
            "> 開く",
            "--- 区切り",
            "# タグを付ける",
            "a|b を比べる",
            "^ を含む",
            "[1] 番目",
            "mail@例 に送る",
        ];

        for text in 名前 {
            let form = Form {
                name: text.to_string(),
                key: String::new(),
                ..見本()
            };
            let parsed = 通す(&form);
            assert!(!parsed.has_error(), "{}: {}", text, 診断(&parsed));
            assert_eq!(parsed.config.apps.len(), 1, "{}", text);
            assert_eq!(parsed.config.apps[0].name, text);
        }
    }

    /// パスの中の記号も同じように元に戻る
    #[test]
    fn 記号を含むパスが元に戻る() {
        let パス = [
            r"C:\Program Files (x86)\a&b\x.exe",
            r"C:\dir-1\+new\x.exe",
            r"C:\a^b\x.exe",
            r"C:\[1]\x.exe",
        ];

        for text in パス {
            let form = Form {
                app: text.to_string(),
                ..見本()
            };
            let parsed = 通す(&form);
            assert!(!parsed.has_error(), "{}: {}", text, 診断(&parsed));
            assert_eq!(parsed.config.apps[0].path, text);
        }
    }

    /// パス欄は別名を書く欄なので、`@` はそのまま設定へ流す
    #[test]
    fn パスに別名を書ける() {
        let form = Form {
            app: r"@sys\tar.exe".to_string(),
            ..見本()
        };
        let 設定 = format!("@sys = C:\\Windows\\System32\r\n{}", form.to_config());
        let parsed = config::parse(&設定);
        assert!(!parsed.has_error(), "{}", 診断(&parsed));
        assert_eq!(parsed.config.apps[0].path, r"C:\Windows\System32\tar.exe");
    }

    #[test]
    fn 項目の行に書くと見出しが付かない() {
        let form = Form {
            ext_style: ExtStyle::PerItem,
            ..見本()
        };
        let text = form.to_config();
        assert!(!text.contains("[.png .jpg .jpeg]\r\n\r\n"), "{}", text);
        assert!(text.contains("(&Z) [.png .jpg .jpeg] |"), "{}", text);
    }

    /// 見出しが無いと項目だけでは通らないので、貼り先を足して確かめる
    #[test]
    fn 項目の行に書いた拡張子が効く() {
        let form = Form {
            ext_style: ExtStyle::PerItem,
            ..見本()
        };
        let parsed = config::parse(&format!("[folder]\r\n{}", form.to_config()));
        assert!(!parsed.has_error(), "{}", 診断(&parsed));
        // 符号なしなので、貼り先の [folder] を無視して置き換わる
        assert_eq!(parsed.config.apps[0].extensions, [".png", ".jpg", ".jpeg"]);
    }

    #[test]
    fn 新しいサブメニューは親の行から書き出す() {
        let form = Form {
            placement: Placement::NewSubmenu,
            submenu_name: "圧縮".to_string(),
            submenu_key: "A".to_string(),
            ..見本()
        };
        let parsed = 通す(&form);
        assert!(!parsed.has_error(), "{}", 診断(&parsed));
        assert_eq!(parsed.config.apps.len(), 1);
        assert_eq!(parsed.config.apps[0].name, "圧縮 (A)");
        assert_eq!(parsed.config.apps[0].submenu.len(), 1);
        assert_eq!(parsed.config.apps[0].submenu[0].name, "ZIP にまとめる (Z)");
    }

    #[test]
    fn 区切り線は項目と同じ階層に付く() {
        let form = Form {
            placement: Placement::NewSubmenu,
            submenu_name: "圧縮".to_string(),
            separator: true,
            ..見本()
        };
        let parsed = 通す(&form);
        assert!(!parsed.has_error(), "{}", 診断(&parsed));
        let submenu = &parsed.config.apps[0].submenu;
        assert_eq!(submenu.len(), 2);
        assert!(submenu[0].is_separator());
    }

    /// 項目だけに指定があると、区切り線にも同じものを付けないと線だけ残る
    #[test]
    fn 項目の行に書くときは区切り線にも拡張子を付ける() {
        let form = Form {
            ext_style: ExtStyle::PerItem,
            separator: true,
            ..見本()
        };
        assert!(
            form.to_config().contains("--- [.png .jpg .jpeg]"),
            "{}",
            form.to_config()
        );
    }

    /// 同じセクションの中なら継承するので、素の `---` でよい
    #[test]
    fn 見出しに書くときは区切り線に拡張子を付けない() {
        let form = Form {
            separator: true,
            ..見本()
        };
        assert!(
            form.to_config().contains("\r\n---\r\n"),
            "{}",
            form.to_config()
        );
    }

    #[test]
    fn 引数を省くと_p_が渡る() {
        let form = Form {
            args: String::new(),
            ..見本()
        };
        let parsed = 通す(&form);
        assert_eq!(parsed.config.apps[0].args, ["$p"]);
    }

    /// 欄を空にする（行末を `|` で終える）のは、欄ごと省くのとは別の意味
    #[test]
    fn 引数を渡さないと空になる() {
        let form = Form {
            no_args: true,
            ..見本()
        };
        assert!(
            form.to_config().contains("tar.exe |\r\n"),
            "{}",
            form.to_config()
        );
        let parsed = 通す(&form);
        assert!(parsed.config.apps[0].args.is_empty());
    }

    #[test]
    fn 名前付きフィールドを書き出す() {
        let form = Form {
            dir: "$d".to_string(),
            confirm: true,
            confirm_message: "$c 個をまとめます".to_string(),
            icon: r"%SystemRoot%\System32\imageres.dll,165".to_string(),
            admin: true,
            delay: true,
            delay_ms: "300".to_string(),
            wait: true,
            when: WhenKind::Multi,
            all_mode: false,
            ..見本()
        };
        let text = form.to_config().replace("\r\n", "\n");

        assert!(text.contains(" :dir $d\n"), "{}", text);
        assert!(text.contains(" :confirm $c 個をまとめます\n"), "{}", text);
        assert!(
            text.contains(r" :icon %SystemRoot%\System32\imageres.dll,165"),
            "{}",
            text
        );
        assert!(text.contains(" :admin\n"), "{}", text);
        assert!(text.contains(" :delay 300\n"), "{}", text);
        assert!(text.contains(" :wait\n"), "{}", text);
        assert!(text.contains(" :when multi\n"), "{}", text);

        let parsed = config::parse(&form.to_config());
        assert!(!parsed.has_error(), "{}", 診断(&parsed));
        let item = &parsed.config.apps[0];
        assert!(item.admin);
        assert!(item.wait);
        assert_eq!(item.delay, Some(300));
        assert!(item.confirm.is_some());
        assert!(item.icon.is_some());
    }

    /// 値を書かない `:confirm` は仕様として認められている
    #[test]
    fn メッセージの無い確認も書ける() {
        let form = Form {
            confirm: true,
            confirm_message: String::new(),
            ..見本()
        };
        assert!(form.to_config().contains(" :confirm\r\n"));
        let parsed = 通す(&form);
        assert_eq!(parsed.config.apps[0].confirm.as_deref(), Some(""));
    }

    #[test]
    fn いつも表示するなら_when_を書かない() {
        assert!(!見本().to_config().contains(":when"));
    }

    /// 見出しをファイルの途中に貼ると下の項目の対象が変わるので、
    /// 書き方によって案内を変えない
    #[test]
    fn 貼り先の案内は書き方で変わらない() {
        let mut form = 見本();
        let 案内 = form.paste_hint();
        assert!(案内.contains("末尾"), "{}", 案内);
        form.ext_style = ExtStyle::PerItem;
        assert_eq!(form.paste_hint(), 案内);
    }
}
