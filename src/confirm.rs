/*!
入力欄と実行前の確認

**順番は入力欄 → 確認 → 起動。** 確認のメッセージに入力した値を入れられる。
ひとつでもキャンセルされたら何も起動しない（半端な入力のまま走らせない）。
確認の理由が重なっても、聞くのは 1 回で本文に理由を並べる。
*/

use crate::Target;
use crate::config::MenuItem;
use crate::menu::to_wide_string;
use crate::placeholder::{PathPlaceholders, RunContext};
use std::ptr::null_mut;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
/// 項目の中に書かれた入力欄を、重複を除いて書かれた順に集める
///
/// 同じ書き方を 2 か所に置いても聞かれるのは 1 回。`-w $?{幅} -h $?{幅}` のような
/// 書き方が意図どおりになる。
pub fn item_prompts(item: &MenuItem) -> Vec<crate::prompt::Prompt<'_>> {
    let mut found: Vec<crate::prompt::Prompt<'_>> = Vec::new();

    let texts = item
        .args
        .iter()
        .chain(std::iter::once(&item.working_dir))
        .chain(item.confirm.iter());

    for text in texts {
        for prompt in crate::prompt::prompts(text) {
            if !found.iter().any(|found| found.source == prompt.source) {
                found.push(prompt);
            }
        }
    }

    found
}

/// `$?{...}` の答えを集める（すべて答えられたら `true`）
///
/// ひとつでもキャンセルされたら、そこで打ち切って実行しない。半端に入力した
/// ぶんだけで起動すると、意図しない引数でコマンドが走る。
pub(crate) fn ask_prompts(item: &MenuItem, base: &PathPlaceholders, ctx: &RunContext) -> bool {
    for prompt in item_prompts(item) {
        // 説明と既定値の中のプレースホルダーは先に解決する（`$?{$a の新しい名前}`
        // や `$?{幅=$e}` が書ける）。基準は :dir と同じく最初の対象
        let message = base.replace(prompt.message, ctx);
        let default_value = base.replace(prompt.default_value, ctx);

        match crate::prompt::ask(prompt.rule, &message, &default_value) {
            Some(value) => ctx.set_prompt(prompt.source, value),
            None => return false,
        }
    }

    true
}

/// 確認ダイアログの本文に並べる対象の数の上限
const MAX_CONFIRM_TARGETS: usize = 15;

/// 実行前に確認する（実行してよければ `true`）
pub(crate) fn confirm_execution(
    item: &MenuItem,
    targets: &[Target],
    ctx: &RunContext,
    confirm_over: Option<u32>,
) -> bool {
    let Some(body) = confirm_body(item, targets, ctx, confirm_over) else {
        return true;
    };

    // 既定を「いいえ」にする。select-first と Enter で誤って選んだときに、
    // そのまま Enter を続けても実行されないようにするのがこの機能の主眼
    let selected = unsafe {
        MessageBoxW(
            null_mut(),
            to_wide_string(&body).as_ptr(),
            to_wide_string("ExtRun - 確認").as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };

    selected == IDYES
}

/// 確認ダイアログの本文（確認が要らなければ `None`）
///
/// 確認する理由は 3 つある（`:confirm` が書いてある / 起動の数が多い / `:admin` で
/// UAC が繰り返される）が、**ダイアログは 1 枚にまとめて理由を本文に並べる**。
/// 対象の数だけ聞かれても答えが変わらないのと同じで、理由の数だけ聞かれても答えは
/// 変わらない。
///
/// 対象の一覧を必ず添える。「何をするか」はメッセージで分かっても、「何に対して
/// するか」は選び間違えているかもしれない部分なので、目で確かめられるようにする。
///
/// 表示から切り離してあるのは、**理由が重なったときの組み立てを実機なしで
/// 確かめられるようにするため**（`MessageBoxW` はモーダルで、出してしまうと
/// テストから触れない）。
fn confirm_body(
    item: &MenuItem,
    targets: &[Target],
    ctx: &RunContext,
    confirm_over: Option<u32>,
) -> Option<String> {
    let elevation = repeated_elevation(item, targets.len());
    if item.confirm.is_none() && confirm_over.is_none() && elevation.is_none() {
        return None;
    }

    // 見出しは `:confirm` に書かれたメッセージ。無ければ何をするかだけ言う
    let mut body = match item.confirm.as_deref() {
        // メッセージにもプレースホルダーを書ける（基準は :dir と同じく最初の対象）
        Some(message) if !message.is_empty() => {
            PathPlaceholders::from_path(&targets[0].path).replace(message, ctx)
        }
        _ if item.admin => format!("「{}」を管理者として実行します。", item.name),
        _ => format!("「{}」を実行します。", item.name),
    };

    // なぜ聞かれたのかを書く。`:confirm` と違って書いた覚えのない確認なので、
    // 設定の名前と値を出しておかないと、うるさいと思った人が止め方にたどり着けない
    if let Some(threshold) = confirm_over {
        body.push_str(&format!(
            "\n\n対象が {} 件で、まとめて確認する件数（confirm-over = {}）を超えています。",
            targets.len(),
            threshold
        ));
    }

    if let Some(note) = &elevation {
        body.push_str("\n\n");
        body.push_str(note);
    }

    body.push_str(&format!("\n\n対象: {} 件\n", targets.len()));
    for target in targets.iter().take(MAX_CONFIRM_TARGETS) {
        body.push_str(&format!("{}\n", target.path.display()));
    }
    if targets.len() > MAX_CONFIRM_TARGETS {
        body.push_str(&format!(
            "ほか {} 件\n",
            targets.len() - MAX_CONFIRM_TARGETS
        ));
    }

    body.push_str("\n実行しますか?");

    Some(body)
}

/// 個別実行の `:admin` で、UAC が何回出るかを伝える一文（要らなければ `None`）
///
/// 昇格はプロセスごとにしかできないので、対象の数だけ確認が出る。知らずに
/// 10 個選ぶと 10 回聞かれることになるため、押す前に知らせる。
/// `+`（まとめて渡す）なら起動は 1 回なので言わない。
fn repeated_elevation(item: &MenuItem, target_count: usize) -> Option<String> {
    (item.admin && !item.all_mode && target_count >= 2).then(|| {
        format!(
            "管理者として実行するため、ユーザーアカウント制御の確認が {} 回表示されます。\n\
             （途中でキャンセルすると、残りは実行されません）",
            target_count
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use std::path::PathBuf;
    /// 確認ダイアログの本文（`C:\x\1.txt` … を対象にする）
    fn body_of(text: &str, count: usize, confirm_over: Option<u32>) -> Option<String> {
        let config = parse(text).config;
        let targets: Vec<Target> = (1..=count)
            .map(|n| Target::from_path(PathBuf::from(format!("C:\\x\\{}.txt", n))))
            .collect();

        confirm_body(
            &config.apps[0],
            &targets,
            &RunContext::for_test(),
            confirm_over,
        )
    }

    /// 理由が 1 つも無ければ確認しない（これまでどおり黙って起動する）
    #[test]
    fn 理由が無ければ確認しない() {
        assert_eq!(body_of("[.txt]\nA | C:\\a.exe", 100, None), None);
    }

    /// 件数だけが理由のときは、なぜ聞かれたのかと止め方の手がかりを出す
    #[test]
    fn 件数の確認には設定の名前と値が出る() {
        let body = body_of("[.txt]\nA | C:\\a.exe", 21, Some(20)).expect("確認が出る");

        assert!(body.contains("「A」を実行します。"), "{}", body);
        assert!(
            body.contains(
                "対象が 21 件で、まとめて確認する件数（confirm-over = 20）を超えています。"
            ),
            "{}",
            body
        );
        assert!(body.contains("対象: 21 件"), "{}", body);
    }

    /// 一覧は上限で打ち切り、隠れた数を添える（ダイアログが画面に収まらなくなる）
    #[test]
    fn 対象の一覧は打ち切られる() {
        let body = body_of("[.txt]\nA | C:\\a.exe", 21, Some(20)).expect("確認が出る");

        assert!(body.contains("C:\\x\\15.txt"), "{}", body);
        assert!(!body.contains("C:\\x\\16.txt"), "{}", body);
        assert!(body.contains("ほか 6 件"), "{}", body);
    }

    /// 理由が重なっても聞かれるのは 1 回。本文に理由が並ぶ
    #[test]
    fn 理由が重なっても本文は一つ() {
        let body = body_of(
            "[.txt]\nA | C:\\a.exe\n :confirm $n を消します\n :admin",
            21,
            Some(20),
        )
        .expect("確認が出る");

        // 見出しは :confirm のメッセージ（プレースホルダーは最初の対象で解決）
        assert!(body.starts_with("1.txt を消します"), "{}", body);
        assert!(body.contains("confirm-over = 20"), "{}", body);
        assert!(
            body.contains("ユーザーアカウント制御の確認が 21 回表示されます"),
            "{}",
            body
        );
        assert_eq!(body.matches("実行しますか?").count(), 1, "{}", body);
    }

    /// `:confirm` を書いていない `:admin` の項目では、見出しで昇格すると伝える
    #[test]
    fn 管理者の項目は見出しでそう言う() {
        let body = body_of("[.txt]\nA | C:\\a.exe\n :admin", 3, None).expect("確認が出る");
        assert!(
            body.starts_with("「A」を管理者として実行します。"),
            "{}",
            body
        );
    }

    /// `+` は何件でも起動が 1 回なので、UAC の回数を知らせる必要が無い
    #[test]
    fn まとめて渡す項目では昇格の知らせが出ない() {
        assert_eq!(
            body_of("[.txt]\n+ A | C:\\a.exe | $p\n :admin", 21, None),
            None
        );
    }
}
