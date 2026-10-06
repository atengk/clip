//! 常用短语模板引擎 (Snippet Template Engine)，负责动态占位符变量解析与安全求值。
//!
//! @author Ateng
//! @since 2026-10-06

pub use crate::storage::Snippet;
#[cfg(not(windows))]
use std::time::{SystemTime, UNIX_EPOCH};

/// 模板变量上下文，用于提供当前本地时间与剪贴板状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetContext {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub clipboard_text: Option<String>,
}

impl SnippetContext {
    /// 创建自定义时间与剪贴板上下文（用于无头单测确定性验证）
    pub fn new(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        clipboard_text: Option<String>,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            clipboard_text,
        }
    }

    /// 获取当前系统本地时间并构造求值上下文
    pub fn now(clipboard_text: Option<String>) -> Self {
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::SYSTEMTIME;
            use windows::Win32::System::SystemInformation::GetLocalTime;
            let st: SYSTEMTIME = unsafe { GetLocalTime() };
            Self {
                year: st.wYear as i32,
                month: st.wMonth as u32,
                day: st.wDay as u32,
                hour: st.wHour as u32,
                minute: st.wMinute as u32,
                second: st.wSecond as u32,
                clipboard_text,
            }
        }

        #[cfg(not(windows))]
        {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let (year, month, day, hour, minute, second) = timestamp_to_datetime(secs);
            Self {
                year,
                month,
                day,
                hour,
                minute,
                second,
                clipboard_text,
            }
        }
    }
}

/// 基于时间戳计算公历年月日时分秒算法 (用于跨平台通用求值)
#[allow(dead_code)]
fn timestamp_to_datetime(timestamp_secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let second = (timestamp_secs % 60) as u32;
    let total_minutes = timestamp_secs / 60;
    let minute = (total_minutes % 60) as u32;
    let total_hours = total_minutes / 60;
    let hour = (total_hours % 24) as u32;
    let mut days = (total_hours / 24) as i64;

    // 1970-01-01 是星期四
    let mut year = 1970;
    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let days_in_months = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 1;
    for &dim in &days_in_months {
        if days < dim as i64 {
            break;
        }
        days -= dim as i64;
        month += 1;
    }
    let day = (days + 1) as u32;

    (year, month, day, hour, minute, second)
}

/// 短语模板解析引擎
pub struct SnippetEngine;

impl SnippetEngine {
    /// 对短语模板中的动态占位符变量进行求值展开
    ///
    /// 支持的占位符列表：
    /// - `{current_date}`: YYYY-MM-DD
    /// - `{time}`: HH:MM:SS
    /// - `{datetime}`: YYYY-MM-DD HH:MM:SS
    /// - `{year}`: YYYY
    /// - `{month}`: MM
    /// - `{day}`: DD
    /// - `{clipboard}`: 当前剪贴板纯文本内容
    ///
    /// @param template 包含占位符的原始短语模板内容
    /// @param ctx 上下文求值参数
    /// @return 展开替换后的最终文本
    pub fn render(template: &str, ctx: &SnippetContext) -> String {
        // 1. 预先格式化各时间占位符常量
        let date_str = format!("{:04}-{:02}-{:02}", ctx.year, ctx.month, ctx.day);
        let time_str = format!("{:02}:{:02}:{:02}", ctx.hour, ctx.minute, ctx.second);
        let datetime_str = format!("{date_str} {time_str}");
        let year_str = format!("{:04}", ctx.year);
        let month_str = format!("{:02}", ctx.month);
        let day_str = format!("{:02}", ctx.day);
        let clip_str = ctx.clipboard_text.as_deref().unwrap_or("");

        // 2. 依次替换对应动态占位符
        template
            .replace("{datetime}", &datetime_str)
            .replace("{current_date}", &date_str)
            .replace("{time}", &time_str)
            .replace("{year}", &year_str)
            .replace("{month}", &month_str)
            .replace("{day}", &day_str)
            .replace("{clipboard}", clip_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet_render_placeholders() {
        let ctx = SnippetContext::new(2026, 10, 6, 14, 30, 45, Some("剪贴板内容".to_string()));

        let tpl = "日期: {current_date}, 时间: {time}, 完整: {datetime}, 剪贴板: [{clipboard}], 年月日: {year}/{month}/{day}";
        let res = SnippetEngine::render(tpl, &ctx);

        assert_eq!(
            res,
            "日期: 2026-10-06, 时间: 14:30:45, 完整: 2026-10-06 14:30:45, 剪贴板: [剪贴板内容], 年月日: 2026/10/06"
        );
    }

    #[test]
    fn test_snippet_render_empty_clipboard() {
        let ctx = SnippetContext::new(2026, 1, 1, 0, 0, 0, None);
        let tpl = "前缀-{clipboard}-后缀";
        let res = SnippetEngine::render(tpl, &ctx);
        assert_eq!(res, "前缀--后缀");
    }

    #[test]
    fn test_snippet_render_no_placeholders() {
        let ctx = SnippetContext::new(2026, 5, 20, 8, 0, 0, None);
        let tpl = "普通静态文本，无需任何替换";
        let res = SnippetEngine::render(tpl, &ctx);
        assert_eq!(res, tpl);
    }
}
