//! 端末上の表示幅を扱う。
//!
//! Rustの `{:<26}` は**文字数**でパディングする。日本語のラベルや機械名は
//! 1文字が2桁を占めるため、そのまま使うと桁が崩れる。工場の機械名や
//! ラベルが日本語である可能性は高いので、幅計算は表示幅で行う。

use unicode_width::UnicodeWidthStr;

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 表示幅が `w` になるまで右側を空白で埋める。既に超えていれば何も足さない。
pub fn pad_right(s: &str, w: usize) -> String {
    let cur = width(s);
    if cur >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - cur))
    }
}

/// 表示幅が `w` を超えないよう切り詰める。切ったときは末尾を `…` にする。
///
/// **文字の途中で切らない。**全角文字を半分だけ描くと以降の桁が全部ずれる。
pub fn truncate(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }

    // 省略記号の分を空けておく
    let budget = w.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = UnicodeWidthStr::width(c.to_string().as_str());
        if used + cw > budget {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_counts_as_double_width() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("仕分け"), 6);
        assert_eq!(width("毎日レートリミット調整"), 22);
    }

    #[test]
    fn padding_aligns_mixed_scripts_to_the_same_column() {
        // 文字数ベースだとこの2つは揃わない
        let a = pad_right("triage", 12);
        let b = pad_right("仕分けループ", 12);
        assert_eq!(width(&a), width(&b));
    }

    #[test]
    fn padding_leaves_overlong_strings_alone() {
        let s = pad_right("非常に長い機械の名前です", 4);
        assert_eq!(s, "非常に長い機械の名前です");
    }

    #[test]
    fn truncation_never_splits_a_wide_character() {
        // 全角の半分だけ描くと以降の桁が全部ずれる
        let s = truncate("あいうえお", 5);
        assert!(width(&s) <= 5, "幅 {} が上限を超えた", width(&s));
    }

    #[test]
    fn truncation_marks_that_it_cut() {
        assert!(truncate("ai-process:needs-human", 10).ends_with('…'));
        assert_eq!(truncate("短い", 10), "短い");
    }

    #[test]
    fn emoji_is_double_width() {
        // 機械行の先頭に 🔨 を置いている
        assert_eq!(width("🔨"), 2);
        assert_eq!(width(&pad_right("🔨 x", 10)), 10);
    }
}
