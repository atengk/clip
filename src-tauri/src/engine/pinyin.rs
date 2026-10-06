//! 中文拼音转换与首字母提取引擎 (Pinyin Matcher)。
//!
//! 支持中文汉字向首字母简拼与全拼的高性能多音字展开转换，为模糊检索提供索引支持。
//!
//! @author Ateng
//! @since 2026-10-06

use pinyin::ToPinyinMulti;

/// 拼音转换与首字母提取器
pub struct PinyinMatcher;

impl PinyinMatcher {
    /// 获取文本字符的所有拼音读音列表 (小写无声调)
    fn get_char_pinyins(ch: char) -> (Vec<char>, Vec<String>) {
        if let Some(multi_iter) = ch.to_pinyin_multi() {
            let mut initials = Vec::new();
            let mut fulls = Vec::new();
            for p in multi_iter {
                let plain = p.plain();
                if let Some(first) = plain.chars().next() {
                    let c = first.to_ascii_lowercase();
                    if !initials.contains(&c) {
                        initials.push(c);
                    }
                }
                let full = plain.to_ascii_lowercase();
                if !fulls.contains(&full) {
                    fulls.push(full);
                }
            }
            if !initials.is_empty() {
                return (initials, fulls);
            }
        }
        let lower = ch.to_ascii_lowercase();
        (vec![lower], vec![lower.to_string()])
    }

    /// 通用笛卡尔积组合扩展算法，确保长文本所有字符均被追加，只对分支数上限进行截断
    fn expand_combinations<T: AsRef<str>>(
        text: &str,
        extractor: fn(char) -> Vec<T>,
        max_variants: usize,
    ) -> Vec<String> {
        let mut results = vec![String::new()];

        for ch in text.chars() {
            let tokens = extractor(ch);
            let mut next_results = Vec::new();
            for prefix in &results {
                for token in &tokens {
                    let mut s = prefix.clone();
                    s.push_str(token.as_ref());
                    next_results.push(s);
                }
            }
            // 仅对当前变体总数做上限裁剪，确保后续字符持续追加
            if next_results.len() > max_variants {
                next_results.truncate(max_variants);
            }
            results = next_results;
        }

        results
    }

    /// 展开多音字生成所有可能的简拼组合（最大保留 16 种变体）
    pub fn to_all_first_letters(text: &str) -> Vec<String> {
        Self::expand_combinations(
            text,
            |ch| {
                let (initials, _) = Self::get_char_pinyins(ch);
                initials.into_iter().map(|c| c.to_string()).collect()
            },
            16,
        )
    }

    /// 展开多音字生成所有可能的全拼组合（最大保留 16 种变体）
    pub fn to_all_full_pinyin(text: &str) -> Vec<String> {
        Self::expand_combinations(
            text,
            |ch| {
                let (_, fulls) = Self::get_char_pinyins(ch);
                fulls
            },
            16,
        )
    }

    /// 生成空格分隔的首字母索引字符串，包含主要读音与多音字变体
    pub fn to_first_letters_index(text: &str) -> String {
        let list = Self::to_all_first_letters(text);
        list.join(" ")
    }

    /// 生成空格分隔的全拼索引字符串
    pub fn to_full_pinyin_index(text: &str) -> String {
        let list = Self::to_all_full_pinyin(text);
        list.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_first_letters_chinese() {
        let text = "银行卡";
        let list = PinyinMatcher::to_all_first_letters(text);
        assert!(list.contains(&"yhk".to_string()), "必须包含 'yhk' (银行卡)");
        assert!(list.contains(&"yxk".to_string()), "必须包含多音字 'yxk'");

        let index_str = PinyinMatcher::to_first_letters_index(text);
        assert!(index_str.contains("yhk"));

        let wx = PinyinMatcher::to_all_first_letters("微信支付");
        assert!(wx.contains(&"wxzf".to_string()));
    }

    #[test]
    fn test_to_first_letters_long_text_no_truncation() {
        // 测试长中文文本，确保后续字符不会被丢弃
        let text = "银行卡中国工商银行软件开发中心分布式架构";
        let list = PinyinMatcher::to_all_first_letters(text);
        for item in &list {
            assert_eq!(item.chars().count(), text.chars().count(), "长文本拼音字符长度必须与原文严格一致");
        }
    }

    #[test]
    fn test_to_full_pinyin() {
        let text = "银行卡";
        let list = PinyinMatcher::to_all_full_pinyin(text);
        assert!(list.contains(&"yinhangka".to_string()), "必须包含 'yinhangka'");
        assert!(list.contains(&"yinxingka".to_string()), "必须包含 'yinxingka'");

        let index_str = PinyinMatcher::to_full_pinyin_index(text);
        assert!(index_str.contains("yinhangka"));
    }
}
