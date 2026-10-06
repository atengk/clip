//! 剪贴板前端 IPC 命令实现 (TauriCommand)。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::commands::AppState;
use crate::storage::ClipboardEntry;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

/// 获取剪贴板历史记录列表
///
/// @param state 全局应用共享状态
/// @param limit 最大返回条数，默认为 50
/// @return 历史条目列表
#[tauri::command]
pub fn get_history(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<ClipboardEntry>, String> {
    let limit = limit.unwrap_or(50);
    state
        .engine
        .get_entries(limit)
        .map_err(|e| format!("获取历史记录失败: {e}"))
}

/// 基于关键词与拼音模糊检索历史记录
///
/// @param state 全局应用共享状态
/// @param query 搜索词
/// @param limit 最大返回条数，默认为 50
/// @return 匹配的历史条目列表
#[tauri::command]
pub fn search_history(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ClipboardEntry>, String> {
    let limit = limit.unwrap_or(50);
    state
        .engine
        .search_entries(&query, limit)
        .map_err(|e| format!("检索历史记录失败: {e}"))
}


/// 隐藏悬浮面板并将指定条目回填至前台原活动窗口
///
/// 遵循极速回填契约：先隐藏面板释放焦点，再模拟键入 Ctrl+V。
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param id 待回填条目的唯一 ID
#[tauri::command]
pub fn paste_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    // 1. 隐藏当前悬浮窗口并交还系统焦点
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    // 2. 预留 50ms 窗口调度等待，确保系统焦点已稳定切回原前台进程
    thread::sleep(Duration::from_millis(50));

    // 3. 注入系统剪贴板并模拟按键发送
    state
        .engine
        .paste_entry(id)
        .map_err(|e| format!("执行极速回填失败: {e}"))
}

/// 主动隐藏当前窗口
///
/// @param app Tauri 应用程序句柄
#[tauri::command]
pub fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(())
}
