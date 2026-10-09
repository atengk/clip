//! 剪贴板前端 IPC 命令实现 (TauriCommand)。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::commands::AppState;
use crate::engine::backup::BackupManifest;
use crate::engine::incognito::IncognitoStatus;
use crate::engine::queue::{QueueItem, QueueStatus};
use crate::engine::transform::TransformAction;
use crate::storage::{ClipboardEntry, Snippet};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

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

/// 删除指定单条历史记录
///
/// @param state 全局应用共享状态
/// @param id 条目唯一 ID
/// @return 是否成功删除
#[tauri::command]
pub fn delete_entry(
    state: State<'_, AppState>,
    id: i64,
) -> Result<bool, String> {
    state
        .engine
        .delete_entry(id)
        .map_err(|e| format!("删除条目失败: {e}"))
}

/// 批量删除指定历史记录
///
/// @param state 全局应用共享状态
/// @param ids 条目唯一 ID 列表
/// @return 实际删除条数
#[tauri::command]
pub fn delete_entries(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<usize, String> {
    state
        .engine
        .delete_entries(&ids)
        .map_err(|e| format!("批量删除条目失败: {e}"))
}

/// 获取当前历史记录容量上限
#[tauri::command]
pub fn get_history_capacity(state: State<'_, AppState>) -> usize {
    state.engine.get_max_capacity()
}

/// 设置历史记录容量上限 (50 / 200 / 500 / 1000)
#[tauri::command]
pub fn set_history_capacity(state: State<'_, AppState>, capacity: usize) -> Result<(), String> {
    state
        .engine
        .set_max_capacity(capacity)
        .map_err(|e| format!("设置容量上限失败: {e}"))
}



/// 安全隐藏主面板
pub fn safe_hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
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
    // 1. 安全隐藏当前悬浮窗口并交还系统焦点
    safe_hide_main_window(&app);

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
    safe_hide_main_window(&app);

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
    safe_hide_main_window(&app);
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
    safe_hide_main_window(&app);
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
    safe_hide_main_window(&app);
    thread::sleep(Duration::from_millis(50));
    state
        .engine
        .paste_snippet(id)
        .map_err(|e| format!("回填常用短语失败: {e}"))
}

/// 获取队列连贴当前全局状态快照
///
/// @param state 全局应用共享状态
/// @return 队列连贴状态对象
#[tauri::command]
pub fn get_paste_queue_status(
    state: State<'_, AppState>,
) -> Result<QueueStatus, String> {
    Ok(state.engine.get_paste_queue_status())
}

/// 切换队列连贴收集模式激活状态
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @return 切换后的最新队列连贴状态
#[tauri::command]
pub fn toggle_paste_queue(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<QueueStatus, String> {
    let _ = state.engine.toggle_paste_queue();
    let status = state.engine.get_paste_queue_status();
    let _ = app.emit("paste-queue-changed", &status);
    crate::sync_paste_queue_hud_and_shortcut(&app, &state.engine);
    Ok(status)
}

/// 停止并清空连贴队列 (退出连贴模式)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @return 清空后的最新队列状态
#[tauri::command]
pub fn clear_paste_queue(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<QueueStatus, String> {
    state.engine.stop_paste_queue();
    let status = state.engine.get_paste_queue_status();
    let _ = app.emit("paste-queue-changed", &status);
    crate::sync_paste_queue_hud_and_shortcut(&app, &state.engine);
    Ok(status)
}

/// 连贴队列头部弹出一项并回填至外部目标窗口 (FIFO)
///
/// 若出队后队列已空，状态机会自动重置为非激活状态闭环。
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @return 弹出的项；若队列为空则返回 None
#[tauri::command]
pub fn paste_queue_pop(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<QueueItem>, String> {
    let item = state
        .engine
        .paste_queue_pop()
        .map_err(|e| format!("执行连贴出队回填失败: {e}"))?;
    let status = state.engine.get_paste_queue_status();
    let _ = app.emit("paste-queue-changed", &status);
    crate::sync_paste_queue_hud_and_shortcut(&app, &state.engine);
    Ok(item)
}

/// 多选条目合并拼接后回填至目标原活动窗口 (AC-4)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param ids 选中的条目主键 ID 列表
/// @param separator 拼接分隔符 (默认换行符 "\n")
/// @return 最终合并回填的文本字符串
#[tauri::command]
pub fn paste_multiple_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    separator: Option<String>,
) -> Result<String, String> {
    safe_hide_main_window(&app);
    thread::sleep(Duration::from_millis(50));
    let sep = separator.as_deref().unwrap_or("\n");
    state
        .engine
        .paste_multiple_entries(&ids, sep)
        .map_err(|e| format!("执行多选合并回填失败: {e}"))
}

/// 切换隐身模式 (AC-1)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param duration_minutes 隐身持续分钟数（None 表示手动退出，Some(15) 或 Some(60) 表示定时）
/// @return 切换后的最新隐身状态快照
#[tauri::command]
pub fn toggle_incognito(
    app: AppHandle,
    state: State<'_, AppState>,
    duration_minutes: Option<u64>,
) -> Result<IncognitoStatus, String> {
    let _ = state.engine.toggle_incognito(duration_minutes);
    let status = state.engine.get_incognito_status();
    let _ = app.emit("incognito-changed", &status);
    crate::sync_tray_icon_and_menu(&app, &state.engine);

    if let Some(mins) = duration_minutes {
        let app_clone = app.clone();
        let engine_clone = state.engine.clone();
        let delay_secs = mins * 60;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(delay_secs + 1));
            if !engine_clone.is_incognito_active() {
                let current_status = engine_clone.get_incognito_status();
                let _ = app_clone.emit("incognito-changed", &current_status);
                crate::sync_tray_icon_and_menu(&app_clone, &engine_clone);
            }
        });
    }

    Ok(status)
}

/// 查询隐身模式状态快照 (AC-1)
///
/// @param state 全局应用共享状态
/// @return 当前隐身状态对象
#[tauri::command]
pub fn get_incognito_status(
    state: State<'_, AppState>,
) -> Result<IncognitoStatus, String> {
    Ok(state.engine.get_incognito_status())
}

/// 导出完整剪贴板数据及图片至 .clipbak 灾备压缩归档包 (AC-3)
///
/// @param state 全局应用共享状态
/// @param path 目标归档压缩包文件路径
/// @return 导出的元数据清单
#[tauri::command]
pub fn export_backup(
    state: State<'_, AppState>,
    path: String,
) -> Result<BackupManifest, String> {
    let p = PathBuf::from(path);
    state
        .engine
        .export_backup(&p)
        .map_err(|e| format!("导出备份失败: {e}"))
}

/// 从 .clipbak 灾备压缩归档包中解包并完整还原数据 (AC-4)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param path 待导入归档压缩包文件路径
/// @return 还原的元数据清单
#[tauri::command]
pub fn import_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<BackupManifest, String> {
    let p = PathBuf::from(path);
    let manifest = state
        .engine
        .import_backup(&p)
        .map_err(|e| format!("导入备份失败: {e}"))?;
    let _ = app.emit("data-restored", ());
    Ok(manifest)
}

/// 查询系统开机自启配置状态 (AC-2)
///
/// @param state 全局应用共享状态
/// @return 当前开机自启是否已启用
#[tauri::command]
pub fn is_autostart_enabled(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.engine.is_autostart_enabled())
}

/// 设置系统开机静默自启状态 (AC-2)
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param enable 是否开启开机自启
/// @return 最新开机自启状态
#[tauri::command]
pub fn set_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    enable: bool,
) -> Result<bool, String> {
    state
        .engine
        .set_autostart(enable)
        .map_err(|e| format!("设置自启动失败: {e}"))?;
    crate::sync_tray_icon_and_menu(&app, &state.engine);
    Ok(state.engine.is_autostart_enabled())
}

/// 清空所有剪贴板历史记录与图片 Blob
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
#[tauri::command]
pub fn clear_all_history(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .engine
        .clear_history()
        .map_err(|e| format!("清空剪贴板历史失败: {e}"))?;
    let _ = app.emit("data-restored", ());
    Ok(())
}

/// 获取当前配置的全局唤起快捷键
///
/// @param state 全局应用共享状态
/// @return 当前生效的全局快捷键组合字符串 (如 "Alt+V")
#[tauri::command]
pub fn get_global_shortcut(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.engine.get_global_shortcut())
}

/// 动态更新并重新注册全局唤起快捷键
///
/// 校验修饰键规则，注销旧快捷键并注册新快捷键；若冲突则回滚并友好报错。
///
/// @param app Tauri 应用程序句柄
/// @param state 全局应用共享状态
/// @param shortcut 新的快捷键组合字符串 (如 "Ctrl+Shift+V")
/// @return 成功返回生效的快捷键字符串
#[tauri::command]
pub fn set_global_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<String, String> {
    let clean = shortcut.trim();
    if clean.is_empty() {
        return Err("快捷键不能为空".to_string());
    }

    // 防御性校验：必须包含至少一个有效修饰键 (Ctrl / Alt / Shift / Super / Command)
    let lower = clean.to_lowercase();
    let has_modifier = lower.contains("ctrl")
        || lower.contains("alt")
        || lower.contains("shift")
        || lower.contains("super")
        || lower.contains("command")
        || lower.contains("meta");

    if !has_modifier {
        return Err("快捷键必须包含至少一个修饰键 (Ctrl、Alt、Shift 或 Win)".to_string());
    }

    let old_shortcut = state.engine.get_global_shortcut();

    // 动态注册新快捷键 (若被系统独占则在此阶段直接报错)
    crate::update_main_shortcut(&app, clean, Some(&old_shortcut))?;

    // 持久化保存至 SQLite
    state
        .engine
        .set_global_shortcut(clean)
        .map_err(|e| format!("保存快捷键配置失败: {e}"))?;

    let _ = app.emit("global-shortcut-changed", clean);
    Ok(clean.to_string())
}

/// 获取当前存储详细状态与磁盘占用情况 (Issue #19)
///
/// @param state 全局应用共享状态
/// @return 存储信息结构体
#[tauri::command]
pub fn get_storage_info(
    state: State<'_, AppState>,
) -> Result<crate::engine::StorageInfo, String> {
    state
        .engine
        .get_storage_info()
        .map_err(|e| format!("获取存储状态失败: {e}"))
}

/// 在操作系统文件资源管理器中打开数据存储目录 (Issue #19)
///
/// @param app Tauri 应用程序句柄
#[tauri::command]
pub fn open_storage_dir(app: AppHandle) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(".clip"));
    let _ = std::fs::create_dir_all(&app_data_dir);

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(&app_data_dir)
            .spawn()
            .map_err(|e| format!("打开存储目录失败: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(&app_data_dir)
            .spawn()
            .map_err(|e| format!("打开存储目录失败: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&app_data_dir)
            .spawn()
            .map_err(|e| format!("打开存储目录失败: {e}"))?;
    }
    Ok(())
}

/// 弹出系统原生“文件另存为”对话框，返回选定的文件绝对路径 (Issue #19)
///
/// @param default_name 建议的默认文件名
/// @return 用户确认的绝对路径或取消操作时返回 None
#[tauri::command]
pub fn select_backup_save_path(default_name: Option<String>) -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        let filename = default_name.unwrap_or_else(|| {
            format!("Clip-Backup-{}.clipbak", get_current_date_stamp())
        });
        let script = format!(
            r#"$ErrorActionPreference = 'SilentlyContinue'; [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; $dialog = New-Object System.Windows.Forms.SaveFileDialog; $dialog.Filter = 'Clip 备份归档 (*.clipbak)|*.clipbak|所有文件 (*.*)|*.*'; $dialog.FileName = '{}'; if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $dialog.FileName }}"#,
            filename
        );
        run_powershell_dialog(&script, "调起文件另存为对话框失败")
    }
    #[cfg(not(windows))]
    {
        let _ = default_name;
        Ok(None)
    }
}

/// 弹出系统原生“打开文件”选择器，返回用户选中的备份文件绝对路径 (Issue #19)
///
/// @return 用户选中的绝对路径或取消操作时返回 None
#[tauri::command]
pub fn select_backup_open_path() -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        let script = r#"$ErrorActionPreference = 'SilentlyContinue'; [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; $dialog = New-Object System.Windows.Forms.OpenFileDialog; $dialog.Filter = 'Clip 备份归档 (*.clipbak)|*.clipbak|所有文件 (*.*)|*.*'; if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { Write-Output $dialog.FileName }"#;
        run_powershell_dialog(script, "调起文件打开对话框失败")
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}

/// 执行轻量 Win32 Forms 对话框 PowerShell 脚本辅助函数
#[cfg(windows)]
fn run_powershell_dialog(script: &str, error_prefix: &str) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{error_prefix}: {e}"))?;
    let res = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if res.is_empty() {
        Ok(None)
    } else {
        Ok(Some(res))
    }
}

/// 基于辛纳公历算法将 UNIX 时间戳转换为 YYYYMMDD 格式紧凑日期戳
fn get_current_date_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    days_to_ymd_string(secs / 86400)
}

/// 天数转换为 YYYYMMDD 格式化字符串纯函数 (方便单测)
fn days_to_ymd_string(days: u64) -> String {
    // 算法基于 Howard Hinnant 的民用历算法，自 1970-01-01 起算
    const ERA_DAYS: u64 = 146097; // 400 年为一个周期周期，共 146097 天
    const OFFSET_TO_ERA: u64 = 719468; // 0000-03-01 至 1970-01-01 的累计天数
    let z = days + OFFSET_TO_ERA;
    let era = z / ERA_DAYS;
    let doe = z - era * ERA_DAYS;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}", year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_days_to_ymd_string_epoch_and_future() {
        // 1970-01-01: 第 0 天
        assert_eq!(days_to_ymd_string(0), "19700101");
        // 2000-01-01: 闰年第 10957 天
        assert_eq!(days_to_ymd_string(10957), "20000101");
        // 2026-10-08: 第 20734 天
        assert_eq!(days_to_ymd_string(20734), "20261008");
    }
}


