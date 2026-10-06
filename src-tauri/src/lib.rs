//! clip 应用程序核心装配与生命周期初始化入口。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod commands;
pub mod engine;
pub mod pal;
pub mod storage;

use crate::commands::clipboard::{get_history, hide_window, paste_entry};
use crate::commands::AppState;
use crate::engine::ClipboardEngine;
use crate::pal::PlatformDriver;
use crate::storage::sqlite::SqliteStorage;
use std::fs;
use std::str::FromStr;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 1. 初始化平台驱动 (Windows 原生驱动，非 Windows 平台降级为 Mock 驱动以保障 CI 跨平台编译)
            #[cfg(windows)]
            let driver: Arc<dyn PlatformDriver> =
                Arc::new(crate::pal::windows::WindowsPlatformDriver::new());
            #[cfg(not(windows))]
            let driver: Arc<dyn PlatformDriver> =
                Arc::new(crate::pal::mock::MockPlatformDriver::new());

            // 2. 初始化持久化存储 (SQLite)
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(".clip"));
            let _ = fs::create_dir_all(&app_data_dir);
            let db_path = app_data_dir.join("clip.db");

            let storage = Arc::new(
                SqliteStorage::new(db_path)
                    .expect("初始化本地 SQLite 存储失败"),
            );

            // 3. 构建核心业务引擎
            let engine = Arc::new(ClipboardEngine::new(driver.clone(), storage));
            app.manage(AppState {
                engine: engine.clone(),
            });

            // 4. 启动系统剪贴板监听器并绑定前端广播事件 (PlatformEvent)
            let app_handle = app.handle().clone();
            let engine_for_monitor = engine.clone();
            driver
                .start_monitor(Arc::new(move || {
                    if let Ok(Some(entry)) = engine_for_monitor.handle_clipboard_change() {
                        let _ = app_handle.emit("clipboard-changed", entry);
                    }
                }))
                .expect("启动系统剪贴板监控失败");

            // 5. 注册全局唤起快捷键 Alt + V
            if let Ok(shortcut) = Shortcut::from_str("Alt+V") {
                let app_handle_for_shortcut = app.handle().clone();
                let _ = app
                    .global_shortcut()
                    .on_shortcut(shortcut, move |_app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            if let Some(window) = app_handle_for_shortcut.get_webview_window("main") {
                                if let Ok(is_visible) = window.is_visible() {
                                    if is_visible {
                                        let _ = window.hide();
                                    } else {
                                        // 1. 唤起前记录当前前台窗口句柄 (Windows)
                                        #[cfg(windows)]
                                        crate::pal::windows::WindowsPlatformDriver::capture_foreground_window();

                                        // 2. 在当前活动显示器中央偏上 (WindowAnchor) 精准定位
                                        if let Ok(Some(monitor)) = window.current_monitor() {
                                            let monitor_size = monitor.size();
                                            let monitor_pos = monitor.position();
                                            let win_size = window
                                                .outer_size()
                                                .unwrap_or(tauri::PhysicalSize::new(640, 460));
                                            let x = monitor_pos.x
                                                + ((monitor_size.width as i32 - win_size.width as i32) / 2);
                                            let y = monitor_pos.y
                                                + ((monitor_size.height as i32 - win_size.height as i32) / 4);
                                            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                                        } else {
                                            let _ = window.center();
                                        }

                                        let _ = window.show();
                                        let _ = window.set_focus();
                                        let _ = app_handle_for_shortcut.emit("panel-shown", ());
                                    }
                                }
                            }
                        }
                    });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_history,
            paste_entry,
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
