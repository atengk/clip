//! 剪贴板前端 IPC 命令实现 (TauriCommand)。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::commands::AppState;
use crate::engine::transform::TransformAction;
use crate::storage::{ClipboardEntry, Snippet};
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

/// 切换指定条目的置顶固定状态 (Pin / Unpin)
///
/// @param state 全局应用共享状态
/// @param id 条目唯一 ID
/// @return 切换后的置顶状态
#[tauri::command]
pub fn toggle_pin(
    state: State<'_, AppState>,
    id: i64,
) -> Result<bool, String> {
    state
        .engine
        .toggle_pin(id)
        .map_err(|e| format!("更新置顶状态失败: {e}"))
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

/// 针对指定条目执行格式清洗与转换并回填至前台原活动窗口
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param id 待回填条目的唯一 ID
/// @param action 格式清洗与转换动作类型
/// @return 转换后的文本结果
#[tauri::command]
pub fn transform_and_paste_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    action: TransformAction,
) -> Result<String, String> {
    // 1. 先行计算转换结果并严格校验（若为非法 JSON 等立即返回错误，窗口绝不提前隐藏）
    let transformed = state
        .engine
        .transform_entry(id, action)
        .map_err(|e| format!("{e}"))?;

    // 2. 校验成功后隐藏当前悬浮窗口并交还系统焦点
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    // 3. 预留 50ms 窗口调度等待，确保系统焦点已稳定切回原前台进程
    thread::sleep(Duration::from_millis(50));

    // 4. 将转换后的文本注入系统剪贴板并模拟按键发送
    state
        .engine
        .paste_text(&transformed)
        .map_err(|e| format!("执行极速回填失败: {e}"))?;

    Ok(transformed)
}

/// 强制以纯文本格式回填指定条目至前台原活动窗口 (Shift + Enter 专用)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param id 待回填条目的唯一 ID
#[tauri::command]
pub fn paste_plain_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    transform_and_paste_entry(app, state, id, TransformAction::PlainText)
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

/// 获取指定图片哈希的 Base64 详细数据（含图片尺寸与文件大小）
///
/// @param state 全局应用共享状态
/// @param hash 图片内容 SHA-256 哈希值
/// @return 包含 Base64 Data URL、尺寸与文件大小的详情模型
#[tauri::command]
pub fn get_image_detail(
    state: State<'_, AppState>,
    hash: String,
) -> Result<crate::engine::ImageDetail, String> {
    state
        .engine
        .get_image_detail(&hash)
        .map_err(|e| format!("获取图片详情失败: {e}"))
}

/// 对指定图片条目执行原生离线 OCR 文字提取
///
/// @param state 全局应用共享状态
/// @param id 目标条目 ID
/// @return 提取出的文本内容
#[tauri::command]
pub fn ocr_image_entry(
    state: State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    state
        .engine
        .ocr_entry(id)
        .map_err(|e| format!("文字提取失败: {e}"))
}

/// 隐藏悬浮面板并将任意文本内容回填至前台原活动窗口（用于 OCR 提取文本回填）
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param text 待回填的目标文本
#[tauri::command]
pub fn paste_custom_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    thread::sleep(Duration::from_millis(50));
    state
        .engine
        .paste_text(&text)
        .map_err(|e| format!("执行回填失败: {e}"))
}

/// 获取全部常用短语模板列表
///
/// @param state 全局应用共享状态
/// @return 短语列表
#[tauri::command]
pub fn get_snippets(state: State<'_, AppState>) -> Result<Vec<Snippet>, String> {
    state
        .engine
        .get_all_snippets()
        .map_err(|e| format!("获取常用短语失败: {e}"))
}

/// 保存常用短语模板 (若指定 id 则更新，否则新建)
///
/// @param state 全局应用共享状态
/// @param id 目标短语主键 ID (可选)
/// @param title 短语标题
/// @param content 短语模板内容
/// @param shortcut 快捷缩写
/// @return 保存后的短语实体
#[tauri::command]
pub fn save_snippet(
    state: State<'_, AppState>,
    id: Option<i64>,
    title: String,
    content: String,
    shortcut: String,
) -> Result<Snippet, String> {
    state
        .engine
        .save_snippet(id, &title, &content, &shortcut)
        .map_err(|e| format!("保存常用短语失败: {e}"))
}

/// 删除指定常用短语模板
///
/// @param state 全局应用共享状态
/// @param id 短语主键 ID
/// @return 是否删除成功
#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    state
        .engine
        .delete_snippet(id)
        .map_err(|e| format!("删除常用短语失败: {e}"))
}

/// 检索常用短语模板
///
/// @param state 全局应用共享状态
/// @param query 检索关键词或前缀 (如 "/meet")
/// @return 匹配的短语列表
#[tauri::command]
pub fn search_snippets(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Snippet>, String> {
    state
        .engine
        .search_snippets(&query)
        .map_err(|e| format!("检索常用短语失败: {e}"))
}

/// 隐藏悬浮面板并将展开后的短语模板内容回填至前台原活动窗口 (AC-2)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param id 短语主键 ID
/// @return 展开后的渲染文本
#[tauri::command]
pub fn paste_snippet(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<String, String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    thread::sleep(Duration::from_millis(50));
    state
        .engine
        .paste_snippet(id)
        .map_err(|e| format!("回填常用短语失败: {e}"))
}

