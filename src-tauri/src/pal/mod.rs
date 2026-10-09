//! 平台抽象层 (Platform Abstraction Layer) 核心契约与错误模型。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod anchor;
pub mod mock;
#[cfg(windows)]
pub mod windows;
#[cfg(target_os = "macos")]
pub mod macos;

use std::sync::Arc;
use thiserror::Error;

/// 平台抽象层统一错误模型
#[derive(Debug, Error)]
pub enum PalError {
    #[error("剪贴板操作失败: {0}")]
    ClipboardError(String),
    #[error("模拟输入注入失败: {0}")]
    InputSimulationError(String),
    #[error("平台监控监听初始化失败: {0}")]
    MonitorError(String),
    #[error("平台驱动内部异常: {0}")]
    InternalError(String),
}

/// 平台驱动统一抽象契约 (PlatformDriver)
///
/// 隔离不同操作系统（Windows/macOS/Linux）底层剪贴板与模拟按键实现，
/// 并为无头单元测试提供确定性测试接缝。
pub trait PlatformDriver: Send + Sync {
    /// 读取系统剪贴板当前纯文本内容
    ///
    /// @return 若剪贴板为空或非文本格式返回 Ok(None)，成功读取返回 Ok(Some(String))
    fn read_text(&self) -> Result<Option<String>, PalError>;

    /// 向系统剪贴板写入纯文本
    ///
    /// @param text 待写入的纯文本字符串
    fn write_text(&self, text: &str) -> Result<(), PalError>;

    /// 向上一个焦点窗口模拟发送粘贴快捷键 (Ctrl+V 或 Cmd+V)
    fn send_paste(&self) -> Result<(), PalError>;

    /// 启动剪贴板变更监听循环，发生变更时触发传入的回调闭包
    ///
    /// @param callback 剪贴板变更时的回调函数
    fn start_monitor(&self, callback: Arc<dyn Fn() + Send + Sync>) -> Result<(), PalError>;

    /// 检查系统剪贴板是否携带密码管理器等私有排除标记 (如 Clipboard Viewer Ignore)
    ///
    /// @return 若存在忽略标记返回 Ok(true)，否则返回 Ok(false)
    fn is_clipboard_ignored(&self) -> Result<bool, PalError>;

    /// 获取触发当前剪贴板复制事件的来源进程名称 (如 "1password.exe")
    ///
    /// @return 进程名，未知时返回 Ok(None)
    fn get_clipboard_source_process(&self) -> Result<Option<String>, PalError>;

    /// 读取系统剪贴板当前图片二进制数据 (标准 BMP 格式)
    ///
    /// @return 若剪贴板无有效图片返回 Ok(None)，成功读取返回 Ok(Some(Vec<u8>))
    fn read_image(&self) -> Result<Option<Vec<u8>>, PalError>;

    /// 向系统剪贴板写入图片位图
    ///
    /// @param data 待写入的图片字节切片 (BMP 格式)
    fn write_image(&self, data: &[u8]) -> Result<(), PalError>;

    /// 对图片二进制数据执行原生离线 OCR 字符提取
    ///
    /// @param data 图片字节切片 (BMP 格式)
    /// @return 提取出的文本字符串
    fn ocr_image(&self, data: &[u8]) -> Result<String, PalError>;
}

