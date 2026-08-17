/*!
対象に合う項目だけを残す

拡張子の解決はパース時に完了しているので、ここでは `MenuItem::extensions` を
見るだけでよい。フィルタで穴が空いたセパレーターの整理も合わせて行う。
*/

use crate::config::MenuItem;
use crate::Target;
use std::collections::HashSet;
/// メニュー項目をフィルタリング
pub fn filter_menu_items(apps: &[MenuItem], targets: &[Target]) -> Vec<MenuItem> {
    let target_info = TargetInfo::from_targets(targets);
    if target_info.file_types.is_empty() {
        return Vec::new();
    }
    filter_with_info(apps, &target_info)
}

/// ターゲット判定用の前処理情報
struct TargetInfo {
    has_folder: bool,
    has_non_folder: bool,
    file_types: HashSet<String>,
}

impl TargetInfo {
    fn from_targets(targets: &[Target]) -> Self {
        let mut has_folder = false;
        let mut has_non_folder = false;
        let mut file_types = HashSet::with_capacity(targets.len());

        for target in targets {
            if target.file_type == "folder" {
                has_folder = true;
            } else {
                has_non_folder = true;
            }
            file_types.insert(target.file_type.clone());
        }

        TargetInfo {
            has_folder,
            has_non_folder,
            file_types,
        }
    }
}

/// 対象に合う項目だけを残す（拡張子はパース時に解決済み）
fn filter_with_info(apps: &[MenuItem], target_info: &TargetInfo) -> Vec<MenuItem> {
    let mut menu_items = Vec::with_capacity(apps.len());

    for app in apps {
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
fn is_menu_item_applicable(extensions: &[String], target_info: &TargetInfo) -> bool {
    if extensions.is_empty() {
        return true;
    }

    if target_info.has_non_folder && extensions.iter().any(|ext| ext == "file") {
        return true;
    }

    if target_info.has_folder && extensions.iter().any(|ext| ext == "folder") {
        return true;
    }

    extensions
        .iter()
        .any(|ext| target_info.file_types.contains(ext))
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
    use crate::config::{parse, Config};
    use std::path::PathBuf;
    /// 実際の設定ファイルを読む（テスト用フィクスチャ兼サンプル）
    fn sample_config() -> Config {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/extrun-config.txt"))
                .expect("extrun-config.txt を読める");

        let parsed = parse(&text);
        let errors: Vec<String> = parsed
            .errors()
            .map(|d| format!("{}行目: {}", d.line, d.message))
            .collect();
        assert!(errors.is_empty(), "設定ファイルのエラー: {:?}", errors);
        parsed.config
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
            (".png", 26),
            (".jpg", 26),
            (".gif", 29),
            (".ico", 25),
            (".bmp", 26),
            (".tif", 27),
            (".mp3", 20),
            (".wav", 20),
            (".mp4", 20),
            (".mkv", 20),
            (".zip", 20),
            (".tar", 20),
            (".gz", 20),
            (".cab", 18),
            (".txt", 20),
            (".md", 20),
            (".csv", 20),
            // [@テキスト] には無いが「文字数・行数を数える」が [+.ps1] で足している
            // （その項目が出るぶん、[file] 冒頭の --- も先頭でなくなり残る）
            (".ps1", 18),
            // どのセクションにも該当しない拡張子は [file] と [file folder] だけ
            (".pdf", 16),
            ("file", 16),
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

    #[test]
    fn 複数選択では和集合になる() {
        let config = sample_config();
        let menu = filter_menu_items(&config.apps, &[target(".txt"), target(".png")]);
        let names: Vec<&str> = menu.iter().map(|item| item.name.as_str()).collect();
        assert!(names.contains(&"メモ帳で開く (N)"));
        assert!(names.contains(&"画像のサイズを調べる"));
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
