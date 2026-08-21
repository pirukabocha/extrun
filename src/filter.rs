/*!
対象に合う項目だけを残す

拡張子の解決はパース時に完了しているので、ここでは `MenuItem::extensions` を
見るだけでよい。フィルタで穴が空いたセパレーターの整理も合わせて行う。

**複数の対象を選んだときは、そのすべてに当てはまる項目だけを残す（積集合）。**
`resolve_invocations` は項目を選んだ対象すべてに対して起動するので、
一部にしか当てはまらない項目を出すと、当てはまらない対象までそのコマンドに
渡ることになる（動画と画像を選んで「JPEG に変換」を押せば動画も渡る）。
*/

use crate::Target;
use crate::config::MenuItem;
use std::collections::HashSet;
/// メニュー項目をフィルタリング
pub fn filter_menu_items(apps: &[MenuItem], targets: &[Target]) -> Vec<MenuItem> {
    let target_info = TargetInfo::from_targets(targets);
    if target_info.file_types.is_empty() {
        return Vec::new();
    }
    filter_with_info(apps, &target_info)
}

/// 残った項目が 0 件だったときの案内
///
/// **種類を混ぜて選んだときは、絞り込みの規則そのものを書く。** 1 種類ずつなら
/// 出ていた項目が消えているので、規則を知らないと設定ファイルの不備に見える。
/// メニュー（`menu.rs`）と `--preview` の両方がこの文言を使う。
pub fn empty_menu_message(targets: &[Target]) -> String {
    let kinds = TargetInfo::from_targets(targets).file_types.len();

    if kinds < 2 {
        return "対象となるファイルに適用できるメニュー項目がありません。".to_string();
    }

    "選んだファイルすべてに適用できるメニュー項目がありません。\n\
     種類の違うものを混ぜて選んだときは、そのすべてに当てはまる項目だけが表示されます。"
        .to_string()
}

/// ターゲット判定用の前処理情報
struct TargetInfo {
    /// 選ばれた対象の種類（重複を潰したもの）
    ///
    /// 項目を出すかどうかは種類だけで決まるので、同じ拡張子を何個選んでも
    /// 判定は 1 回で済む。
    file_types: HashSet<String>,
    /// 選ばれた数（`:when` の判定に使う）
    count: usize,
}

impl TargetInfo {
    fn from_targets(targets: &[Target]) -> Self {
        let mut file_types = HashSet::with_capacity(targets.len());

        for target in targets {
            file_types.insert(target.file_type.clone());
        }

        TargetInfo {
            file_types,
            count: targets.len(),
        }
    }
}

/// 対象に合う項目だけを残す（拡張子はパース時に解決済み）
fn filter_with_info(apps: &[MenuItem], target_info: &TargetInfo) -> Vec<MenuItem> {
    let mut menu_items = Vec::with_capacity(apps.len());

    for app in apps {
        // 選んだ数による出し分け（`:when`）。拡張子と同じ「出すかどうか」の
        // 条件なので、サブメニューの親にもセパレーターにも同じように効く
        if app
            .when
            .is_some_and(|when| !when.matches(target_info.count))
        {
            continue;
        }

        if app.has_submenu() {
            // 子が 1 つも残らなかったサブメニューは丸ごと落とす
            let filtered_submenu = filter_with_info(&app.submenu, target_info);
            if !filtered_submenu.is_empty() {
                let mut new_app = app.clone();
                new_app.submenu = filtered_submenu;
                menu_items.push(new_app);
            }
        } else if is_menu_item_applicable(&app.extensions, target_info) {
            menu_items.push(app.clone());
        }
    }

    cleanup_separators(menu_items)
}

/// メニュー項目が対象に適用可能か判定
///
/// **選ばれた対象のすべてに当てはまるときだけ出す。** 1 つでも外れるものが
/// あれば出さない（出せば、その対象までコマンドに渡ってしまう）。
fn is_menu_item_applicable(extensions: &[String], target_info: &TargetInfo) -> bool {
    if extensions.is_empty() {
        return true;
    }

    target_info
        .file_types
        .iter()
        .all(|file_type| covers(extensions, file_type))
}

/// 1 つの種類を項目の対象指定が含んでいるか
///
/// 拡張子そのもののほか、フォルダなら `folder`、それ以外なら `file` でも当たる。
/// 拡張子の無いファイルの種類はもともと `file` なので、この 2 つは重なる。
fn covers(extensions: &[String], file_type: &str) -> bool {
    let generic = if file_type == "folder" {
        "folder"
    } else {
        "file"
    };

    extensions
        .iter()
        .any(|ext| ext == file_type || ext == generic)
}

/// セパレーターをクリーンアップ
fn cleanup_separators(items: Vec<MenuItem>) -> Vec<MenuItem> {
    // 先頭のセパレーターをスキップ
    let first_non_separator = items
        .iter()
        .position(|item| !item.is_separator())
        .unwrap_or(items.len());

    // 連続するセパレーターを1つにまとめる
    let mut filtered = Vec::with_capacity(items.len().saturating_sub(first_non_separator));
    let mut prev_separator = false;

    for item in items.into_iter().skip(first_non_separator) {
        if item.is_separator() {
            if !prev_separator {
                filtered.push(item);
                prev_separator = true;
            }
        } else {
            filtered.push(item);
            prev_separator = false;
        }
    }

    // 末尾のセパレーターを削除
    if filtered.last().is_some_and(|item| item.is_separator()) {
        filtered.pop();
    }

    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, parse};
    use std::path::PathBuf;
    /// 設定ファイルを読んでパースする（エラーがあればその場で落とす）
    fn config_from_file(relative: &str) -> Config {
        let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), relative);
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} を読める: {e}"));

        let parsed = parse(&text);
        let errors: Vec<String> = parsed
            .errors()
            .map(|d| format!("{}行目: {}", d.line, d.message))
            .collect();
        assert!(errors.is_empty(), "{path} のエラー: {:?}", errors);
        parsed.config
    }

    /// 書式を一通り使ったフィクスチャ
    ///
    /// かつては同梱のサンプル設定そのものだったが、サンプルは初めて開く人向けに
    /// 最小限まで削ったので、検証用としてこちらに残してある（配布物には入らない）。
    fn sample_config() -> Config {
        config_from_file("tests/fixtures/full-config.txt")
    }

    fn target(file_type: &str) -> Target {
        Target {
            file_type: file_type.to_string(),
            path: PathBuf::from("C:\\dummy\\sample"),
        }
    }

    /// セパレーターとサブメニューの中身も含めた項目数
    fn count_items(items: &[MenuItem]) -> usize {
        items
            .iter()
            .map(|item| 1 + count_items(&item.submenu))
            .sum()
    }

    fn menu_for(config: &Config, file_type: &str) -> Vec<MenuItem> {
        filter_menu_items(&config.apps, &[target(file_type)])
    }

    #[test]
    fn 対象ごとの項目数が期待どおり() {
        // extrun-config.txt から構築されるメニューの項目数
        // （セパレーターとサブメニューの中身も数える）
        let expected = [
            (".png", 28),
            (".jpg", 28),
            (".gif", 31),
            (".ico", 27),
            (".bmp", 28),
            (".tif", 29),
            (".mp3", 22),
            (".wav", 22),
            (".mp4", 22),
            (".mkv", 22),
            (".zip", 22),
            (".tar", 22),
            (".gz", 22),
            (".cab", 20),
            (".txt", 22),
            (".md", 22),
            (".csv", 22),
            // [@テキスト] には無いが「文字数・行数を数える」が [+.ps1] で足している
            // （その項目が出るぶん、[file] 冒頭の --- も先頭でなくなり残る）
            (".ps1", 20),
            // どのセクションにも該当しない拡張子は [file] と [file folder] だけ
            (".pdf", 18),
            ("file", 18),
            ("folder", 22),
        ];

        let config = sample_config();
        let mut mismatches = Vec::new();

        for (file_type, count) in expected {
            let actual = count_items(&menu_for(&config, file_type));
            if actual != count {
                mismatches.push(format!("{}: 期待 {} / 実際 {}", file_type, count, actual));
            }
        }

        assert!(mismatches.is_empty(), "項目数の不一致: {:#?}", mismatches);
    }

    /// 同梱するサンプル設定
    ///
    /// 初めて開く人が最初に目にするファイルなので、**中身が増えていないこと**を
    /// 見張る（`config_from_file` が書式エラーも検出する）。書き足したくなったら
    /// フィクスチャの方（`tests/fixtures/full-config.txt`）かレシピ集へ。
    #[test]
    fn 同梱のサンプル設定は最小限のまま() {
        let config = config_from_file("extrun-config.txt");

        let expected = [
            // 画像 2 つ + [file] の「親フォルダを開いて選択」
            (".png", 3),
            (".jpg", 3),
            // テキスト 1 つ + [file]
            (".txt", 2),
            // どのセクションにも該当しない拡張子は [file] だけ
            (".pdf", 1),
            ("file", 1),
            ("folder", 2),
        ];

        for (file_type, count) in expected {
            assert_eq!(
                count_items(&menu_for(&config, file_type)),
                count,
                "{file_type} の項目数"
            );
        }
    }

    /// 選んだ数による出し分け（`:when`）
    #[test]
    fn 選んだ数で項目を出し分ける() {
        let config = parse(
            "[.txt]\n\
             いつでも | C:\\a.exe\n\
             1 つのとき | C:\\a.exe\n :when single\n\
             複数のとき | C:\\a.exe\n :when multi",
        )
        .config;

        let names = |count: usize| -> Vec<String> {
            let targets: Vec<Target> = (0..count).map(|_| target(".txt")).collect();
            filter_menu_items(&config.apps, &targets)
                .iter()
                .map(|item| item.name.clone())
                .collect()
        };

        assert_eq!(names(1), vec!["いつでも", "1 つのとき"]);
        assert_eq!(names(2), vec!["いつでも", "複数のとき"]);
        assert_eq!(names(9), vec!["いつでも", "複数のとき"]);
    }

    /// サブメニューの親に書けば中身ごと消える（拡張子と同じ扱い）
    #[test]
    fn サブメニューの親にも効く() {
        let config = parse(
            "[.txt]\n\
             まとめ処理 | \n :when multi\n\
             > 子 | C:\\a.exe",
        )
        .config;

        assert!(filter_menu_items(&config.apps, &[target(".txt")]).is_empty());
        assert_eq!(
            filter_menu_items(&config.apps, &[target(".txt"), target(".txt")]).len(),
            1
        );
    }

    #[test]
    fn 先頭のセパレーターは取り除かれる() {
        // file は [file] セクションの先頭セパレーターが最初の項目になる
        let config = sample_config();
        let menu = menu_for(&config, "file");
        assert!(!menu[0].is_separator());
        assert_eq!(menu[0].name, "親フォルダを開いて選択 (S)");
        assert!(!menu.last().expect("項目がある").is_separator());
    }

    #[test]
    fn jpg_のメニュー構造() {
        let config = sample_config();
        let menu = menu_for(&config, ".jpg");
        let names: Vec<&str> = menu.iter().map(|item| item.name.as_str()).collect();

        assert_eq!(
            names,
            vec![
                "開く (O)",
                "画像のサイズを調べる",
                "形式を変換 (C)",
                "長辺 1280px に縮小する",
                "長辺を指定して縮小する",
                "---",
                "親フォルダを開いて選択 (S)",
                "読み取り専用・隠し属性を解除",
                "SHA256 を書き出す",
                "ハッシュ値を選んで書き出す",
                "名前を変えて複製する",
                "---",
                "サイズを調べる",
                "---",
                "圧縮 (Z)",
                "---",
                "パスをコピーする (P)",
            ]
        );

        // [-.jpg -.jpeg] と [.gif] の子は落ち、末尾に残るセパレーターも消える
        let convert = &menu[2];
        assert_eq!(convert.name, "形式を変換 (C)");
        let children: Vec<&str> = convert
            .submenu
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(children, vec!["PNG に変換", "BMP に変換"]);
    }

    #[test]
    fn folder_のサブメニューにセパレーターが残る() {
        let config = sample_config();
        let menu = menu_for(&config, "folder");
        let open = &menu[0];
        assert_eq!(open.name, "開く (D)");
        let children: Vec<&str> = open.submenu.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            children,
            vec![
                "エクスプローラで開く (E)",
                "---",
                "PowerShell で開く (P)",
                "コマンドプロンプトで開く (C)",
                "管理者としてコマンドプロンプトを開く (A)",
            ]
        );
        // 引数欄を空にした項目は引数なし、:dir はプレースホルダーを保ったまま
        assert!(open.submenu[3].args.is_empty());
        assert_eq!(open.submenu[3].working_dir, "$p");
        // :admin が付くのは最後の 1 つだけ
        assert!(!open.submenu[3].admin);
        assert!(open.submenu[4].admin);
    }

    /// 種類の違うものを混ぜて選んだら、**すべてに当てはまる項目だけ**が残る
    ///
    /// 項目は選んだ対象すべてに対して起動されるので、片方にしか当てはまらない
    /// 項目を出すと、当てはまらない側までそのコマンドに渡ってしまう
    #[test]
    fn 混ぜて選ぶと共通する項目だけ残る() {
        let config = sample_config();
        let menu = filter_menu_items(&config.apps, &[target(".txt"), target(".png")]);
        let names: Vec<&str> = menu.iter().map(|item| item.name.as_str()).collect();

        // どちらにも当てはまる [file] / [file folder] の項目は残る
        assert!(names.contains(&"パスをコピーする (P)"), "{:?}", names);
        assert!(names.contains(&"サイズを調べる"), "{:?}", names);
        // 片方にしか当てはまらない項目は消える
        assert!(!names.contains(&"メモ帳で開く (N)"), "{:?}", names);
        assert!(!names.contains(&"画像のサイズを調べる"), "{:?}", names);
    }

    /// 同じ種類だけを何個選んでも、その種類向けの項目はそのまま出る
    ///
    /// 絞り込むのは種類が混ざったときだけで、数は関係ない（数で変わるのは
    /// `:when` を書いた項目だけ）
    #[test]
    fn 同じ種類だけなら絞り込まれない() {
        let config = sample_config();
        let targets = [target(".png"), target(".png"), target(".png")];
        let names: Vec<String> = filter_menu_items(&config.apps, &targets)
            .iter()
            .map(|item| item.name.clone())
            .collect();

        assert!(
            names.contains(&"画像のサイズを調べる".to_string()),
            "{:?}",
            names
        );
        assert!(names.contains(&"形式を変換 (C)".to_string()), "{:?}", names);
    }

    /// 別々の拡張子でも、項目の対象指定が両方を含んでいれば残る
    #[test]
    fn 両方を含む指定なら残る() {
        let config = parse(
            "[.mp4 .jpg]\n\
             両方に出る | C:\\a.exe\n\
             動画だけ [.mp4] | C:\\a.exe\n\
             画像だけ [.jpg] | C:\\a.exe\n\
             すべてのファイル [file] | C:\\a.exe",
        )
        .config;

        let names: Vec<String> = filter_menu_items(&config.apps, &[target(".mp4"), target(".jpg")])
            .iter()
            .map(|item| item.name.clone())
            .collect();

        assert_eq!(names, vec!["両方に出る", "すべてのファイル"]);
    }

    /// `file` はフォルダに当たらないので、ファイルとフォルダを混ぜると
    /// `[file folder]` の項目だけが残る
    #[test]
    fn ファイルとフォルダを混ぜると両対応の項目だけ残る() {
        let config = parse(
            "[file]\n\
             ファイルだけ | C:\\a.exe\n\
             フォルダだけ [folder] | C:\\a.exe\n\
             どちらでも [file folder] | C:\\a.exe",
        )
        .config;

        let targets = [target(".txt"), target("folder")];
        let names: Vec<String> = filter_menu_items(&config.apps, &targets)
            .iter()
            .map(|item| item.name.clone())
            .collect();

        assert_eq!(names, vec!["どちらでも"]);
    }

    /// 0 件の案内は、種類を混ぜたときだけ絞り込みの規則を説明する
    #[test]
    fn 項目が無いときの案内() {
        assert!(
            empty_menu_message(&[target(".mp4")]).starts_with("対象となるファイル"),
            "1 種類なら従来の文言"
        );
        assert!(
            empty_menu_message(&[target(".mp4"), target(".mp4")]).starts_with("対象となるファイル"),
            "同じ種類が並んでも 1 種類は 1 種類"
        );
        assert!(
            empty_menu_message(&[target(".mp4"), target(".jpg")]).contains("すべてに当てはまる"),
            "混ぜたときは絞り込みの規則を書く"
        );
    }

    /// 共通する項目が 1 つも無ければメニューは空になる（呼び出し側が案内を出す）
    #[test]
    fn 共通する項目が無ければ空になる() {
        let config = parse(
            "[.mp4]\n\
             動画だけ | C:\\a.exe\n\
             [.jpg]\n\
             画像だけ | C:\\a.exe",
        )
        .config;

        let targets = [target(".mp4"), target(".jpg")];
        assert!(filter_menu_items(&config.apps, &targets).is_empty());
    }

    #[test]
    fn セクションの指定は絞り込みではない() {
        // [folder] セクションの項目でも [file folder] と書けばファイルにも出る
        let config = sample_config();
        for file_type in ["folder", ".txt", ".png"] {
            let names: Vec<String> = menu_for(&config, file_type)
                .iter()
                .map(|item| item.name.clone())
                .collect();
            assert!(
                names.iter().any(|n| n == "サイズを調べる"),
                "{} に出ていない",
                file_type
            );
        }
    }

    #[test]
    fn まとめて実行の指定が読める() {
        let config = sample_config();
        let menu = menu_for(&config, "folder");
        let compress = menu
            .iter()
            .find(|item| item.name == "圧縮 (Z)")
            .expect("圧縮がある");
        // 親が Z、子も Z。キーはメニューごとに独立しているので衝突しない
        let zip = compress
            .submenu
            .iter()
            .find(|item| item.name == "ZIP")
            .expect("ZIP がある");
        assert_eq!(compress.accesskey_char(), Some('Z'));
        assert_eq!(zip.accesskey_char(), Some('Z'));
        let single = &zip.submenu[0];
        let batch = &zip.submenu[1];
        assert_eq!(single.name, "個別に圧縮 (S)");
        assert!(!single.all_mode);
        assert!(batch.all_mode);
    }
}
