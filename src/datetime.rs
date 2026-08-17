/*!
日時プレースホルダー `$t{...}` の書式

`$t` 単独では何も起きない。**`$t{` が続いたときだけ**書式として解釈する。
既存の設定ファイルに素の `$t` があっても意味が変わらないようにするため。

書式文字は PowerShell の `Get-Date -Format` と同じ .NET の記法に合わせてある。
曜日だけは .NET がロケール依存になるところを、日本語（`ddd`）と英語（`EEE`）で
書き分ける形にした。同じ設定ファイルがどの環境でも同じ文字列を出さないと、
ファイル名に混ぜたときに別の PC で結果が変わってしまう。

中括弧の中に書けるのは**書式文字と英字以外の文字だけ**。`$t{yyyy年MM月dd日}` は
通るが、`$t{yyyyMMdd_backup}` はエラーになる（`$t{yyyyMMdd}_backup` と外に出す）。
リテラル用の引用記法を増やさずに、書式の書き間違いを `--check` で拾えるようにする
ための割り切り。
*/

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// 曜日（`SYSTEMTIME.wDayOfWeek` は 0 が日曜）
const WEEKDAY_JA_SHORT: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
const WEEKDAY_JA: [&str; 7] = [
    "日曜日",
    "月曜日",
    "火曜日",
    "水曜日",
    "木曜日",
    "金曜日",
    "土曜日",
];
const WEEKDAY_EN_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_EN: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// 使える書式文字（エラーメッセージ用）
const FORMAT_LETTERS: &str = "y M d H m s E";

/// 現地時刻
#[derive(Debug, Clone, Copy)]
pub struct LocalTime {
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    /// 0 が日曜
    day_of_week: u16,
}

impl LocalTime {
    /// 現在の現地時刻を取る
    pub fn now() -> Self {
        let mut system_time: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut system_time) };

        LocalTime {
            year: system_time.wYear,
            month: system_time.wMonth,
            day: system_time.wDay,
            hour: system_time.wHour,
            minute: system_time.wMinute,
            second: system_time.wSecond,
            day_of_week: system_time.wDayOfWeek,
        }
    }

    /// 書式文字列を展開する
    pub fn format(&self, spec: &str) -> String {
        let mut out = String::with_capacity(spec.len() + 8);

        for run in runs(spec) {
            match run {
                Run::Literal(text) => out.push_str(text),
                Run::Letter(letter, count) => match self.field(letter, count) {
                    Some(value) => out.push_str(&value),
                    // 書式として解釈できない並びはそのまま残す。パース時に
                    // エラーにしているので、ここに来るのは検証を通さずに
                    // 呼んだときだけ
                    None => out.extend(std::iter::repeat_n(letter as char, count)),
                },
            }
        }

        out
    }

    /// 書式文字とその繰り返し数に対応する値
    ///
    /// **有効な書き方の一覧はここだけ**。`validate_spec` もこの関数を呼んで
    /// 判定するので、対応表が 2 か所に分かれてずれることがない。
    fn field(&self, letter: u8, count: usize) -> Option<String> {
        let weekday = (self.day_of_week as usize).min(6);

        let value = match (letter, count) {
            (b'y', 2) => format!("{:02}", self.year % 100),
            (b'y', 4) => self.year.to_string(),
            (b'M', 1) => self.month.to_string(),
            (b'M', 2) => format!("{:02}", self.month),
            (b'd', 1) => self.day.to_string(),
            (b'd', 2) => format!("{:02}", self.day),
            (b'd', 3) => WEEKDAY_JA_SHORT[weekday].to_string(),
            (b'd', 4) => WEEKDAY_JA[weekday].to_string(),
            (b'E', 3) => WEEKDAY_EN_SHORT[weekday].to_string(),
            (b'E', 4) => WEEKDAY_EN[weekday].to_string(),
            (b'H', 1) => self.hour.to_string(),
            (b'H', 2) => format!("{:02}", self.hour),
            (b'm', 1) => self.minute.to_string(),
            (b'm', 2) => format!("{:02}", self.minute),
            (b's', 1) => self.second.to_string(),
            (b's', 2) => format!("{:02}", self.second),
            _ => return None,
        };

        Some(value)
    }
}

/// テスト用の固定時刻（2026-08-15 土曜 14:03:05）
///
/// `placeholder.rs` のテストからも使う（`LocalTime` の中身は非公開なので、
/// 決まった時刻を組み立てられるのはこのモジュールだけ）。
#[cfg(test)]
pub fn test_time() -> LocalTime {
    LocalTime {
        year: 2026,
        month: 8,
        day: 15,
        hour: 14,
        minute: 3,
        second: 5,
        day_of_week: 6,
    }
}

/// 書式文字列を分解した断片
enum Run<'a> {
    /// 同じ ASCII 英字が続いたもの（文字とその個数）
    Letter(u8, usize),
    /// それ以外（区切り記号や日本語）
    Literal(&'a str),
}

/// 書式文字列を「同じ英字の連なり」と「それ以外」に分ける
///
/// `yyyy-MM-dd` なら y×4 / `-` / M×2 / `-` / d×2 になる。
fn runs(spec: &str) -> Vec<Run<'_>> {
    let bytes = spec.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let letter = bytes[i];
        if !letter.is_ascii_alphabetic() {
            // 英字が来るまでをひとかたまりにする（多バイト文字もここに入る）
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push(Run::Literal(&spec[start..i]));
            continue;
        }

        let start = i;
        while i < bytes.len() && bytes[i] == letter {
            i += 1;
        }
        out.push(Run::Letter(letter, i - start));
    }

    out
}

/// `}` の位置を探す
fn find_close(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|b| *b == b'}')
}

/// 文字列に含まれる `$t{...}` を検証し、最初に見つかった問題を返す
///
/// 引数と作業フォルダはエスケープを解決する前に渡ってくるので、`^$` は飛ばす。
pub fn validate(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'^' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }

        if bytes[i] == b'$' && bytes[i + 1..].starts_with(b"t{") {
            let start = i + 3;
            let Some(end) = find_close(&bytes[start..]) else {
                return Some("$t{ が } で閉じられていません".to_string());
            };
            if let Some(message) = validate_spec(&text[start..start + end]) {
                return Some(message);
            }
            i = start + end + 1;
            continue;
        }

        i += 1;
    }

    None
}

/// 中括弧の中身を検証する
fn validate_spec(spec: &str) -> Option<String> {
    if spec.is_empty() {
        return Some("$t{} に書式がありません（例: $t{yyyyMMdd}）".to_string());
    }

    // 判定は LocalTime::field に任せる（対応表を 2 か所に分けないため）。
    // 値は捨てるので中身は何でもよく、検証は --check とパース時にしか走らない
    let sample = LocalTime {
        year: 2000,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        day_of_week: 0,
    };

    for run in runs(spec) {
        let Run::Letter(letter, count) = run else {
            continue;
        };
        if sample.field(letter, count).is_none() {
            return Some(message_for(letter, count));
        }
    }

    None
}

/// 書式として解釈できなかった並びの説明
fn message_for(letter: u8, count: usize) -> String {
    let repeated: String = std::iter::repeat_n(letter as char, count).collect();

    match letter {
        b'y' => format!("$t{{}} の年は yy か yyyy で書きます: {}", repeated),
        b'M' => format!("$t{{}} の月は M か MM で書きます: {}", repeated),
        b'd' => format!(
            "$t{{}} の d は d / dd（日）、ddd / dddd（曜日）です: {}",
            repeated
        ),
        b'E' => format!("$t{{}} の英語の曜日は EEE か EEEE で書きます: {}", repeated),
        b'H' => format!("$t{{}} の時は H か HH で書きます: {}", repeated),
        b'm' => format!("$t{{}} の分は m か mm で書きます: {}", repeated),
        b's' => format!("$t{{}} の秒は s か ss で書きます: {}", repeated),
        b'h' => format!(
            "$t{{}} で 12 時間制は使えません。HH（24 時間制）を使ってください: {}",
            repeated
        ),
        // check.rs は 1 行 1 件で桁を揃えて出すので、改行を入れない
        _ => format!(
            "$t{{}} に書式ではない英字があります: {}（使えるのは {}。日付以外の文字は $t{{yyyyMMdd}}_backup のように中括弧の外に書きます）",
            repeated, FORMAT_LETTERS
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-08-15（土）14:03:05
    fn 見本() -> LocalTime {
        test_time()
    }

    #[test]
    fn 年月日と時分秒を桁で書き分けられる() {
        let t = 見本();
        assert_eq!(t.format("yyyy"), "2026");
        assert_eq!(t.format("yy"), "26");
        assert_eq!(t.format("M"), "8");
        assert_eq!(t.format("MM"), "08");
        assert_eq!(t.format("d"), "15");
        assert_eq!(t.format("dd"), "15");
        assert_eq!(t.format("H"), "14");
        assert_eq!(t.format("HH"), "14");
        assert_eq!(t.format("m"), "3");
        assert_eq!(t.format("mm"), "03");
        assert_eq!(t.format("s"), "5");
        assert_eq!(t.format("ss"), "05");
    }

    #[test]
    fn 組み合わせて書ける() {
        let t = 見本();
        assert_eq!(t.format("yyyyMMdd"), "20260815");
        assert_eq!(t.format("yyyy-MM-dd"), "2026-08-15");
        assert_eq!(t.format("yyMMdd_HHmm"), "260815_1403");
        assert_eq!(t.format("HHmmss"), "140305");
    }

    /// 英字でない文字はそのまま通るので、和暦まじりの書き方ができる
    #[test]
    fn 日本語をそのまま通す() {
        let t = 見本();
        assert_eq!(t.format("yyyy年MM月dd日"), "2026年08月15日");
        assert_eq!(t.format("yyyy/MM/dd HH:mm:ss"), "2026/08/15 14:03:05");
    }

    #[test]
    fn 曜日は日本語と英語を書き分けられる() {
        let t = 見本();
        assert_eq!(t.format("ddd"), "土");
        assert_eq!(t.format("dddd"), "土曜日");
        assert_eq!(t.format("EEE"), "Sat");
        assert_eq!(t.format("EEEE"), "Saturday");
        assert_eq!(t.format("yyyyMMdd(ddd)"), "20260815(土)");
    }

    /// d は文字数で意味が変わる（.NET と同じ）。取り違えやすいので明示的に押さえる
    #[test]
    fn d_は文字数で日と曜日が入れ替わる() {
        let t = 見本();
        assert_eq!(t.format("dd"), "15");
        assert_eq!(t.format("ddd"), "土");
    }

    #[test]
    fn 曜日は日曜から土曜まで対応する() {
        let mut t = 見本();
        for (index, expected) in ["日", "月", "火", "水", "木", "金", "土"]
            .iter()
            .enumerate()
        {
            t.day_of_week = index as u16;
            assert_eq!(&t.format("ddd"), expected);
        }
    }

    // -----------------------------------------------------------------
    // 検証
    // -----------------------------------------------------------------

    #[test]
    fn 正しい書式は問題なしになる() {
        for text in [
            "$t{yyyyMMdd}",
            "$-p_$t{yyyy-MM-dd}.zip",
            "$t{yyyy年MM月dd日(ddd)}",
            "-o $t{HHmmss} $p",
            // $t 単独は書式ではないので素通りする
            "$t",
            "100$t 円",
        ] {
            assert_eq!(validate(text), None, "{}", text);
        }
    }

    #[test]
    fn 閉じ忘れを検出する() {
        let message = validate("$t{yyyyMMdd").expect("エラーになる");
        assert!(message.contains("閉じられていません"), "{}", message);
    }

    #[test]
    fn 空の書式を検出する() {
        let message = validate("$t{}").expect("エラーになる");
        assert!(message.contains("書式がありません"), "{}", message);
    }

    #[test]
    fn 書式ではない英字を検出する() {
        let message = validate("$t{yyyyMMdd_backup}").expect("エラーになる");
        assert!(message.contains("書式ではない英字"), "{}", message);
        // 逃げ道を案内する
        assert!(message.contains("中括弧の外"), "{}", message);
    }

    #[test]
    fn 桁の間違いを検出する() {
        for (text, 含む) in [
            ("$t{y}", "yy か yyyy"),
            ("$t{yyy}", "yy か yyyy"),
            ("$t{ddddd}", "ddd / dddd"),
            ("$t{E}", "EEE か EEEE"),
            ("$t{HHH}", "H か HH"),
        ] {
            let message = validate(text).unwrap_or_else(|| panic!("{} はエラーになる", text));
            assert!(message.contains(含む), "{} → {}", text, message);
        }
    }

    /// 12 時間制は用意していないので、24 時間制を案内する
    #[test]
    fn 十二時間制は案内付きで断る() {
        let message = validate("$t{hh:mm}").expect("エラーになる");
        assert!(message.contains("HH（24 時間制）"), "{}", message);
    }

    /// 引数は `^` を残したまま渡ってくるので、エスケープされたものは対象外
    #[test]
    fn エスケープされた書式は検証しない() {
        assert_eq!(validate("^$t{これは書式ではない}"), None);
    }

    #[test]
    fn 複数の書式のうち後ろの間違いも見つける() {
        let message = validate("$t{yyyyMMdd}_$t{QQ}").expect("エラーになる");
        assert!(message.contains("QQ"), "{}", message);
    }
}
