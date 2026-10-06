//! Tauri IPC 命令分发层。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod clipboard;

use crate::engine::ClipboardEngine;
use std::sync::Arc;

/// 全局共享应用状态 (AppState)
pub struct AppState {
    /// 核心剪贴板状态机引擎
    pub engine: Arc<ClipboardEngine>,
}
