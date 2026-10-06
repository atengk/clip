//! clip 应用程序核心装配与生命周期初始化入口。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod commands;
pub mod engine;
pub mod pal;
pub mod storage;

use crate::commands::clipboard::{
    clear_paste_queue, delete_snippet, get_history, get_image_detail, get_paste_queue_status,
    get_snippets, hide_window, ocr_image_entry, paste_custom_text, paste_entry,
    paste_multiple_entries, paste_plain_entry, paste_queue_pop, paste_snippet, save_snippet,
    search_history, search_snippets, toggle_paste_queue, toggle_pin, transform_and_paste_entry,
};
use crate::commands::AppState;
use crate::engine::ClipboardEngine;
use crate::pal::PlatformDriver;
use crate::storage::sqlite::SqliteStorage;
use std::fs;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// 同步连贴模式下的全局 Control+V 拦截快捷键与屏幕右下角 HUD 胶囊窗口生命周期 (AC-1, AC-2, AC-3)
///
/// 仅在连贴模式激活且队列存在待出队项时注册 Control+V；
/// 队列清空或退出模式时立即注销交还系统焦点并自动隐藏右下角 HUD 胶囊窗口。
pub fn sync_paste_queue_hud_and_shortcut(app_handle: &tauri::AppHandle, engine: &Arc<ClipboardEngine>) {
    let status = engine.get_paste_queue_status();

    // 1. 同步屏幕右下角 Capsule HUD 窗口显示状态 (AC-1 & AC-3)
    if let Some(hud_win) = app_handle.get_webview_window("hud") {
        if status.is_active {
            if let Ok(Some(monitor)) = hud_win.current_monitor() {
                let monitor_size = monitor.size();
                let monitor_pos = monitor.position();
                let x = monitor_pos.x + monitor_size.width as i32 - 280;
                let y = monitor_pos.y + monitor_size.height as i32 - 80;
                let _ = hud_win.set_position(tauri::PhysicalPosition::new(x, y));
            }
            let _ = hud_win.show();
        } else {
            let _ = hud_win.hide();
        }
    }

    // 2. 动态注册或注销 Control+V 全局连贴拦截 (AC-2)
    if let Ok(v_shortcut) = Shortcut::from_str("Control+V") {
        let is_registered = app_handle.global_shortcut().is_registered(v_shortcut);
        if status.is_active && status.count > 0 {
            if !is_registered {
                let engine_for_v = engine.clone();
                let _ = app_handle.global_shortcut().on_shortcut(
                    v_shortcut,
                    move |app, _sc, event| {
                        if event.state() == ShortcutState::Pressed {
                            let app_clone = app.clone();
                            let engine_clone = engine_for_v.clone();
                            std::thread::spawn(move || {
                                if let Ok(v_sc) = Shortcut::from_str("Control+V") {
                                    let _ = app_clone.global_shortcut().unregister(v_sc);
                                }
                                std::thread::sleep(Duration::from_millis(20));
                                if let Ok(Some(_)) = engine_clone.paste_queue_pop() {
                                    let st = engine_clone.get_paste_queue_status();
                                    let _ = app_clone.emit("paste-queue-changed", &st);
                                }
                                std::thread::sleep(Duration::from_millis(60));
                                sync_paste_queue_hud_and_shortcut(&app_clone, &engine_clone);
                            });
                        }
                    },
                );
            }
        } else if is_registered {
            let _ = app_handle.global_shortcut().unregister(v_shortcut);
        }
    }
}

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
            let blob_dir = app_data_dir.join("blobs");
            fs::create_dir_all(&blob_dir).expect("创建本地图片 Blob 目录失败");

            let engine = Arc::new(
                ClipboardEngine::new(driver.clone(), storage)
                    .with_blob_dir(blob_dir),
            );
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
                        let queue_status = engine_for_monitor.get_paste_queue_status();
                        let _ = app_handle.emit("paste-queue-changed", &queue_status);
                        sync_paste_queue_hud_and_shortcut(&app_handle, &engine_for_monitor);
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

            // 6. 注册连贴收集模式全局切换快捷键 Alt + Shift + C (AC-1)
            if let Ok(shortcut_queue) = Shortcut::from_str("Alt+Shift+C") {
                let app_handle_for_queue = app.handle().clone();
                let engine_for_queue = engine.clone();
                let _ = app
                    .global_shortcut()
                    .on_shortcut(shortcut_queue, move |_app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            let _ = engine_for_queue.toggle_paste_queue();
                            let status = engine_for_queue.get_paste_queue_status();
                            let _ = app_handle_for_queue.emit("paste-queue-changed", &status);
                            sync_paste_queue_hud_and_shortcut(&app_handle_for_queue, &engine_for_queue);
                        }
                    });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_history,
            search_history,
            paste_entry,
            paste_plain_entry,
            transform_and_paste_entry,
            hide_window,
            toggle_pin,
            get_image_detail,
            ocr_image_entry,
            paste_custom_text,
            get_snippets,
            save_snippet,
            delete_snippet,
            search_snippets,
            paste_snippet,
            get_paste_queue_status,
            toggle_paste_queue,
            clear_paste_queue,
            paste_queue_pop,
            paste_multiple_entries
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
