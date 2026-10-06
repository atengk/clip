//! 剪贴板文本格式清洗与常用格式转换器 (TextTransform)。
//!
//! 提供一键去除前后空白/多余换行、大小写转换、下划线/驼峰转换、JSON 语法美化压缩与纯文本净化。
//!
//! @author Ateng
//! @since 2026-10-06

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 格式转换执行异常枚举
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransformError {
    #[error("非合法 JSON 语法格式: {0}")]
    InvalidJson(String),
}

/// 支持的文本清洗与转换动作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformAction {
    /// 去除首尾空白字符与连续多余空行
    Trim,
    /// 强制转换为纯文本并规范化换行
    PlainText,
    /// 全部转换为大写 (UPPERCASE)
    Uppercase,
    /// 全部转换为小写 (lowercase)
    Lowercase,
    /// 转换为蛇形下划线命名 (snake_case)
    SnakeCase,
    /// 转换为小驼峰命名 (camelCase)
    CamelCase,
    /// JSON 语法美化排版 (2 空格缩进)
    JsonPrettify,
    /// JSON 紧凑压缩 (单行 Minify)
    JsonMinify,
}

/// 文本清洗与格式转换器
pub struct TextTransformer;

impl TextTransformer {
    /// 执行指定的文本清洗与转换动作
    ///
    /// @param input 原始文本内容
    /// @param action 转换动作类型
    /// @return 转换后的新文本或转换错误
    pub fn transform(input: &str, action: TransformAction) -> Result<String, TransformError> {
        match action {
            TransformAction::Trim => Ok(Self::trim_and_compact(input)),
            TransformAction::PlainText => Ok(Self::to_plain_text(input)),
            TransformAction::Uppercase => Ok(input.to_uppercase()),
            TransformAction::Lowercase => Ok(input.to_lowercase()),
            TransformAction::SnakeCase => Ok(Self::to_snake_case(input)),
            TransformAction::CamelCase => Ok(Self::to_camel_case(input)),
            TransformAction::JsonPrettify => Self::json_prettify(input),
            TransformAction::JsonMinify => Self::json_minify(input),
        }
    }

    /// 去除前后空白，修剪每行行尾空白，并将连续多个空行折叠为单个空行
    fn trim_and_compact(input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();
        let mut previous_was_empty = false;

        for line in trimmed.lines() {
            let line_trimmed = line.trim_end();
            if line_trimmed.trim().is_empty() {
                if !previous_was_empty {
                    lines.push("");
                    previous_was_empty = true;
                }
            } else {
                lines.push(line_trimmed);
                previous_was_empty = false;
            }
        }

        lines.join("\n")
    }

    /// 强制净化为纯文本并过滤不可见特殊控制字符（安全保留原生换行与制表符）
    fn to_plain_text(input: &str) -> String {
        input
            .chars()
            .filter(|&c| c == '\n' || c == '\r' || c == '\t' || !c.is_control())
            .collect()
    }

    /// 转换为蛇形下划线命名 (snake_case)
    fn to_snake_case(input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let mut words = Vec::new();
        let mut current_word = String::new();
        let chars: Vec<char> = trimmed.chars().collect();

        for i in 0..chars.len() {
            let c = chars[i];
            if c == '_' || c == '-' || c.is_whitespace() {
                if !current_word.is_empty() {
                    words.push(current_word.to_lowercase());
                    current_word = String::new();
                }
            } else if c.is_uppercase() {
                // 判断是否是驼峰边界 (例如 aB 或 ABCd 中的 Cd)
                let is_prev_lower = i > 0 && chars[i - 1].is_lowercase();
                let is_next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
                if (is_prev_lower || (!current_word.is_empty() && is_next_lower))
                    && !current_word.is_empty()
                {
                    words.push(current_word.to_lowercase());
                    current_word = String::new();
                }
                current_word.push(c);
            } else {
                current_word.push(c);
            }
        }

        if !current_word.is_empty() {
            words.push(current_word.to_lowercase());
        }

        words.join("_")
    }

    /// 转换为小驼峰命名 (camelCase)
    fn to_camel_case(input: &str) -> String {
        let snake = Self::to_snake_case(input);
        if snake.is_empty() {
            return String::new();
        }

        let parts: Vec<&str> = snake.split('_').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return String::new();
        }

        let mut result = parts[0].to_lowercase();
        for part in &parts[1..] {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                result.extend(chars);
            }
        }

        result
    }

    /// JSON 语法美化排版 (2 空格缩进)
    fn json_prettify(input: &str) -> Result<String, TransformError> {
        let val: serde_json::Value =
            serde_json::from_str(input).map_err(|e| TransformError::InvalidJson(e.to_string()))?;
        serde_json::to_string_pretty(&val).map_err(|e| TransformError::InvalidJson(e.to_string()))
    }

    /// JSON 语法紧凑压缩 (单行 Minify)
    fn json_minify(input: &str) -> Result<String, TransformError> {
        let val: serde_json::Value =
            serde_json::from_str(input).map_err(|e| TransformError::InvalidJson(e.to_string()))?;
        serde_json::to_string(&val).map_err(|e| TransformError::InvalidJson(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_empty_and_whitespace() {
        assert_eq!(
            TextTransformer::transform("", TransformAction::Trim).unwrap(),
            ""
        );
        assert_eq!(
            TextTransformer::transform("   \n\n  \t  ", TransformAction::Trim).unwrap(),
            ""
        );
        assert_eq!(
            TextTransformer::transform("", TransformAction::SnakeCase).unwrap(),
            ""
        );
        assert_eq!(
            TextTransformer::transform("   ", TransformAction::CamelCase).unwrap(),
            ""
        );
    }

    #[test]
    fn test_transform_trim_and_compact() {
        let raw = "   \n\n  Hello World   \n\n\n\n  Line 2   \n\n   ";
        let res = TextTransformer::transform(raw, TransformAction::Trim).unwrap();
        assert_eq!(res, "Hello World\n\n  Line 2");
    }

    #[test]
    fn test_transform_plain_text() {
        let raw = "First\r\nSecond\rThird\x07End";
        let res = TextTransformer::transform(raw, TransformAction::PlainText).unwrap();
        assert_eq!(res, "First\r\nSecond\rThirdEnd");
    }

    #[test]
    fn test_transform_uppercase_and_lowercase() {
        let text = "Hello Clip 2026!";
        assert_eq!(
            TextTransformer::transform(text, TransformAction::Uppercase).unwrap(),
            "HELLO CLIP 2026!"
        );
        assert_eq!(
            TextTransformer::transform(text, TransformAction::Lowercase).unwrap(),
            "hello clip 2026!"
        );
    }

    #[test]
    fn test_transform_snake_case() {
        assert_eq!(
            TextTransformer::transform("helloWorld", TransformAction::SnakeCase).unwrap(),
            "hello_world"
        );
        assert_eq!(
            TextTransformer::transform("Hello-World", TransformAction::SnakeCase).unwrap(),
            "hello_world"
        );
        assert_eq!(
            TextTransformer::transform("UserAPIResponse", TransformAction::SnakeCase).unwrap(),
            "user_api_response"
        );
        assert_eq!(
            TextTransformer::transform("already_snake_case", TransformAction::SnakeCase).unwrap(),
            "already_snake_case"
        );
        assert_eq!(
            TextTransformer::transform("中文测试Variable", TransformAction::SnakeCase).unwrap(),
            "中文测试_variable"
        );
    }

    #[test]
    fn test_transform_camel_case() {
        assert_eq!(
            TextTransformer::transform("hello_world", TransformAction::CamelCase).unwrap(),
            "helloWorld"
        );
        assert_eq!(
            TextTransformer::transform("Hello-World", TransformAction::CamelCase).unwrap(),
            "helloWorld"
        );
        assert_eq!(
            TextTransformer::transform("user_api_response", TransformAction::CamelCase).unwrap(),
            "userApiResponse"
        );
    }

    #[test]
    fn test_transform_json_prettify() {
        let compact_json = r#"{"name":"clip","version":"1.0.0","tags":["tauri","rust"]}"#;
        let prettified = TextTransformer::transform(compact_json, TransformAction::JsonPrettify).unwrap();
        assert!(prettified.contains("{\n"));
        assert!(prettified.contains("  \"name\": \"clip\""));

        let invalid = "not a valid json";
        let err = TextTransformer::transform(invalid, TransformAction::JsonPrettify);
        assert!(matches!(err, Err(TransformError::InvalidJson(_))));
    }

    #[test]
    fn test_transform_json_minify() {
        let pretty_json = "{\n  \"a\": 1,\n  \"b\": [ 2, 3 ]\n}";
        let minified = TextTransformer::transform(pretty_json, TransformAction::JsonMinify).unwrap();
        assert_eq!(minified, r#"{"a":1,"b":[2,3]}"#);
    }
}
