//! 隐私安全过滤器 (Privacy Filter) 与敏感字段防窥打码器 (Masked View)。
//!
//! 负责密码管理器协议级过滤、应用黑名单判定以及手机号、身份证、银行卡与 API Token 等敏感凭据的规则嗅探与脱敏。
//!
//! @author Ateng
//! @since 2026-10-06

/// 隐私安全过滤器
pub struct PrivacyFilter {
    blacklist: Vec<String>,
}

impl PrivacyFilter {
    /// 构造隐私安全过滤器，内置默认密码管理器黑名单
    pub fn new() -> Self {
        Self {
            blacklist: vec![
                "keepass.exe".into(),
                "keepassxc.exe".into(),
                "1password.exe".into(),
                "bitwarden.exe".into(),
                "lastpass.exe".into(),
                "dashlane.exe".into(),
            ],
        }
    }

    /// 使用自定义进程黑名单创建过滤器
    pub fn with_blacklist(blacklist: Vec<String>) -> Self {
        Self { blacklist }
    }

    /// 判定剪贴板事件是否属于需要阻断丢弃的敏感凭据
    ///
    /// @param is_protocol_ignored 是否携带系统级/密码管理器排除标记
    /// @param source_process 当前复制操作的来源进程名称
    /// @return 若属于敏感信息需丢弃返回 true，否则返回 false
    pub fn should_ignore(&self, is_protocol_ignored: bool, source_process: Option<&str>) -> bool {
        // 1. 协议级标记优先拦截 (如 Clipboard Viewer Ignore)
        if is_protocol_ignored {
            return true;
        }

        // 2. 进程黑名单匹配拦截 (不区分大小写)
        if let Some(proc) = source_process {
            let proc_lower = proc.to_lowercase();
            for item in &self.blacklist {
                if item.to_lowercase() == proc_lower {
                    return true;
                }
            }
        }

        false
    }
}

impl Default for PrivacyFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// 敏感信息嗅探与界面防窥打码器 (Masked View)
pub struct MaskedDetector;

impl MaskedDetector {
    /// 针对手机号、身份证号、银行卡号及常见 API Token 执行界面脱敏打码
    ///
    /// @param text 输入原始内容
    /// @return 元组 (脱敏后文本, 是否检测到敏感凭据)
    pub fn mask_text(text: &str) -> (String, bool) {
        let trimmed = text.trim();
        let mut masked = text.to_string();
        let mut detected = false;

        // 1. 独立单值精准脱敏优先匹配
        // (1) 常见 API Token
        if (trimmed.starts_with("sk-") || trimmed.starts_with("ghp_")) && trimmed.len() >= 24 {
            let prefix = if trimmed.starts_with("sk-") { "sk-" } else { "ghp_" };
            let suffix = &trimmed[trimmed.len() - 4..];
            return (format!("{prefix}****{suffix}"), true);
        }

        // (2) 18 位中国大陆身份证号精确脱敏
        if trimmed.len() == 18 {
            let prefix_digits = trimmed[..17].chars().all(|c| c.is_ascii_digit());
            let last_valid = trimmed.chars().last().map(|c| c.is_ascii_digit() || c == 'X' || c == 'x').unwrap_or(false);
            if prefix_digits && last_valid {
                return (format!("{}********{}", &trimmed[..6], &trimmed[14..]), true);
            }
        }

        // (3) 16~19 位银行卡号精确脱敏
        if (16..=19).contains(&trimmed.len()) && trimmed.chars().all(|c| c.is_ascii_digit()) {
            return (format!("{} **** **** {}", &trimmed[..4], &trimmed[trimmed.len() - 4..]), true);
        }

        // (4) 11 位中国大陆手机号精确脱敏
        if trimmed.len() == 11 && trimmed.starts_with('1') && trimmed.chars().all(|c| c.is_ascii_digit()) {
            return (format!("{}****{}", &trimmed[..3], &trimmed[7..]), true);
        }

        // 2. 文本内嵌敏感信息模式替换
        // (1) 内嵌 API Token 替换
        for prefix in &["sk-", "ghp_"] {
            if let Some(pos) = masked.find(prefix) {
                let rest = &masked[pos..];
                let token_len = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').count();
                if token_len >= 24 {
                    let full_token: String = rest.chars().take(token_len).collect();
                    let replacement = format!("{}****{}", prefix, &full_token[full_token.len() - 4..]);
                    masked = masked.replace(&full_token, &replacement);
                    detected = true;
                }
            }
        }

        // (2) 内嵌 18 位身份证模式替换
        let chars: Vec<char> = masked.chars().collect();
        if chars.len() >= 18 {
            let mut i = 0;
            while i + 18 <= chars.len() {
                let is_prefix_boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
                let is_suffix_boundary = i + 18 == chars.len() || !chars[i + 18].is_ascii_alphanumeric();
                let is_first_17_digits = chars[i..i + 17].iter().all(|c| c.is_ascii_digit());
                let is_last_id_char = chars[i + 17].is_ascii_digit() || chars[i + 17] == 'X' || chars[i + 17] == 'x';

                if is_prefix_boundary && is_suffix_boundary && is_first_17_digits && is_last_id_char {
                    let id_str: String = chars[i..i + 18].iter().collect();
                    let replacement = format!("{}********{}", &id_str[..6], &id_str[14..]);
                    masked = masked.replace(&id_str, &replacement);
                    detected = true;
                    i += 18;
                    continue;
                }
                i += 1;
            }
        }

        // (3) 内嵌 11 位手机号模式替换
        let chars_phone: Vec<char> = masked.chars().collect();
        if chars_phone.len() >= 11 {
            let mut i = 0;
            while i + 11 <= chars_phone.len() {
                let is_boundary_start = i == 0 || !chars_phone[i - 1].is_ascii_digit();
                let is_boundary_end = i + 11 == chars_phone.len() || !chars_phone[i + 11].is_ascii_digit();

                if is_boundary_start && is_boundary_end && chars_phone[i] == '1' && chars_phone[i..i + 11].iter().all(|c| c.is_ascii_digit()) {
                    let phone_str: String = chars_phone[i..i + 11].iter().collect();
                    let replacement = format!("{}****{}", &phone_str[..3], &phone_str[7..]);
                    masked = masked.replace(&phone_str, &replacement);
                    detected = true;
                    i += 11;
                    continue;
                }
                i += 1;
            }
        }

        (masked, detected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_privacy_filter_protocol_flag() {
        let filter = PrivacyFilter::new();
        assert!(filter.should_ignore(true, None));
        assert!(filter.should_ignore(true, Some("notepad.exe")));
        assert!(!filter.should_ignore(false, Some("notepad.exe")));
    }

    #[test]
    fn test_privacy_filter_process_blacklist() {
        let filter = PrivacyFilter::new();
        assert!(filter.should_ignore(false, Some("KeePass.exe")));
        assert!(filter.should_ignore(false, Some("1password.exe")));
        assert!(filter.should_ignore(false, Some("bitwarden.exe")));
        assert!(!filter.should_ignore(false, Some("code.exe")));
    }

    #[test]
    fn test_masked_detector_phone() {
        let (masked, is_sensitive) = MaskedDetector::mask_text("13812345678");
        assert!(is_sensitive);
        assert_eq!(masked, "138****5678");

        let (embedded, is_sens2) = MaskedDetector::mask_text("联系方式: 13987654321 请妥善保存");
        assert!(is_sens2);
        assert_eq!(embedded, "联系方式: 139****4321 请妥善保存");
    }

    #[test]
    fn test_masked_detector_id_card() {
        let (masked, is_sensitive) = MaskedDetector::mask_text("110101199003072345");
        assert!(is_sensitive);
        assert_eq!(masked, "110101********2345");

        let (masked_x, is_sens_x) = MaskedDetector::mask_text("11010119900307234X");
        assert!(is_sens_x);
        assert_eq!(masked_x, "110101********234X");

        let (embedded, is_sens_emb) = MaskedDetector::mask_text("身份证: 110101199003072345 (已实名)");
        assert!(is_sens_emb);
        assert_eq!(embedded, "身份证: 110101********2345 (已实名)");
    }

    #[test]
    fn test_masked_detector_bank_card() {
        let (masked, is_sensitive) = MaskedDetector::mask_text("6222021234567890");
        assert!(is_sensitive);
        assert_eq!(masked, "6222 **** **** 7890");
    }

    #[test]
    fn test_masked_detector_api_token() {
        let token = "sk-proj-1234567890abcdef1234567890";
        let (masked, is_sensitive) = MaskedDetector::mask_text(token);
        assert!(is_sensitive);
        assert_eq!(masked, "sk-****7890");

        let gh_token = "ghp_1234567890abcdef1234567890abcdef";
        let (gh_masked, gh_sens) = MaskedDetector::mask_text(gh_token);
        assert!(gh_sens);
        assert_eq!(gh_masked, "ghp_****cdef");

        let embedded_token = "export OPENAI_API_KEY=sk-proj-1234567890abcdef1234567890";
        let (emb_masked, emb_sens) = MaskedDetector::mask_text(embedded_token);
        assert!(emb_sens);
        assert_eq!(emb_masked, "export OPENAI_API_KEY=sk-****7890");
    }

    #[test]
    fn test_masked_detector_regular_text() {
        let (masked, is_sensitive) = MaskedDetector::mask_text("普通的剪贴板文本内容");
        assert!(!is_sensitive);
        assert_eq!(masked, "普通的剪贴板文本内容");
    }
}
