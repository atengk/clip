//! clip 应用程序核心装配与生命周期初始化入口。
//!
//! @author Ateng
//! @since 2026-10-06

pub mod commands;
pub mod engine;
pub mod pal;
pub mod storage;

use crate::commands::clipboard::{
    clear_all_history, clear_paste_queue, delete_entry, delete_entries, delete_snippet, export_backup, get_history,
    get_image_detail, get_incognito_status, get_paste_queue_status, get_snippets, hide_window,
    import_backup, is_autostart_enabled, ocr_image_entry, paste_custom_text, paste_entry,
    paste_multiple_entries, paste_plain_entry, paste_queue_pop, paste_snippet, save_snippet,
    search_history, search_snippets, set_autostart, toggle_incognito, toggle_paste_queue,
    toggle_pin, transform_and_paste_entry, get_global_shortcut, set_global_shortcut,
    get_storage_info, open_storage_dir, select_backup_save_path, select_backup_open_path,
    get_history_capacity, set_history_capacity,
};
use crate::commands::updater::{download_and_install_update, execute_in_place_update};
use crate::commands::AppState;
use crate::engine::ClipboardEngine;
use crate::pal::PlatformDriver;
use crate::storage::sqlite::SqliteStorage;
use std::fs;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
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

/// 构造并刷新系统托盘菜单 (AC-1, AC-2)
pub fn build_tray_menu(
    app: &tauri::AppHandle,
    engine: &Arc<ClipboardEngine>,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let incognito_status = engine.get_incognito_status();
    let autostart_enabled = engine.is_autostart_enabled();

    let show_item = MenuItem::with_id(app, "show", "显示面板 (Alt+V)", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    let incognito_label = if incognito_status.is_active {
        "隐身模式 (已开启)"
    } else {
        "隐身模式 (不记录)"
    };
    let incognito_item = CheckMenuItem::with_id(
        app,
        "toggle_incognito",
        incognito_label,
        true,
        incognito_status.is_active,
        None::<&str>,
    )?;

    let incognito_15m = MenuItem::with_id(app, "incognito_15m", "隐身 15 分钟", true, None::<&str>)?;
    let incognito_1h = MenuItem::with_id(app, "incognito_1h", "隐身 1 小时", true, None::<&str>)?;

    let sep2 = PredefinedMenuItem::separator(app)?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        "toggle_autostart",
        "开机自启动",
        true,
        autostart_enabled,
        None::<&str>,
    )?;

    let clear_item = MenuItem::with_id(app, "clear_history", "清空剪贴板历史", true, None::<&str>)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 clip", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &show_item,
            &sep1,
            &incognito_item,
            &incognito_15m,
            &incognito_1h,
            &sep2,
            &autostart_item,
            &clear_item,
            &sep3,
            &quit_item,
        ],
    )
}

/// 同步托盘图标外观（隐身模式变灰）与菜单状态 (AC-1)
pub fn sync_tray_icon_and_menu(app_handle: &tauri::AppHandle, engine: &Arc<ClipboardEngine>) {
    if let Some(tray) = app_handle.tray_by_id("main_tray") {
        let is_active = engine.is_incognito_active();

        // 1. 同步托盘图标颜色与 Tooltip (AC-1)
        if is_active {
            if let Some(default_icon) = app_handle.default_window_icon() {
                let width = default_icon.width();
                let height = default_icon.height();
                let mut rgba = default_icon.rgba().to_vec();
                for pixel in rgba.chunks_exact_mut(4) {
                    let gray = ((pixel[0] as f32 * 0.299)
                        + (pixel[1] as f32 * 0.587)
                        + (pixel[2] as f32 * 0.114)) as u8;
                    pixel[0] = gray;
                    pixel[1] = gray;
                    pixel[2] = gray;
                    pixel[3] = (pixel[3] as u16 * 150 / 255) as u8;
                }
                let gray_icon = Image::new_owned(rgba, width, height);
                let _ = tray.set_icon(Some(gray_icon));
            }
            let _ = tray.set_tooltip(Some("clip (🕵️ 隐身模式中)"));
        } else {
            if let Some(default_icon) = app_handle.default_window_icon() {
                let _ = tray.set_icon(Some(default_icon.clone()));
            }
            let _ = tray.set_tooltip(Some("clip"));
        }

        // 2. 刷新右键菜单
        if let Ok(menu) = build_tray_menu(app_handle, engine) {
            let _ = tray.set_menu(Some(menu));
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

            // 5. 动态读取并注册全局唤起快捷键 (默认 Alt+V，支持数据库持久化与动态重绑)
            let saved_shortcut = engine.get_global_shortcut();
            if let Err(e) = update_main_shortcut(app.handle(), &saved_shortcut, None) {
                eprintln!("初始化注册用户全局快捷键失败: {e}，正在尝试回退至 Alt+V");
                let _ = update_main_shortcut(app.handle(), "Alt+V", None);
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

            // 7. 装配系统常驻托盘守护与快捷菜单 (AC-1, AC-2)
            let tray_menu = build_tray_menu(app.handle(), &engine)?;
            let default_icon = app
                .default_window_icon()
                .cloned()
                .expect("应用程序缺少默认窗口图标");

            let app_for_tray = app.handle().clone();
            let engine_for_tray = engine.clone();

            let _tray = TrayIconBuilder::with_id("main_tray")
                .icon(default_icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .tooltip("clip")
                .on_tray_icon_event(move |_tray, event| {
                    match event {
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        } => {
                            show_full_window(&app_for_tray);
                        }
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } => {
                            if let Some(window) = app_for_tray.get_webview_window("main") {
                                if let Ok(is_visible) = window.is_visible() {
                                    if is_visible {
                                        crate::commands::clipboard::safe_hide_main_window(&app_for_tray);
                                    } else {
                                        show_full_window(&app_for_tray);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                })
                .on_menu_event(move |app_handle, event| {
                    let id_str = event.id().as_ref();
                    match id_str {
                        "show" => {
                            show_full_window(app_handle);
                        }
                        "toggle_incognito" => {
                            let _ = engine_for_tray.toggle_incognito(None);
                            let status = engine_for_tray.get_incognito_status();
                            let _ = app_handle.emit("incognito-changed", &status);
                            sync_tray_icon_and_menu(app_handle, &engine_for_tray);
                        }
                        "incognito_15m" => {
                            engine_for_tray.enter_incognito(Some(15));
                            let status = engine_for_tray.get_incognito_status();
                            let _ = app_handle.emit("incognito-changed", &status);
                            sync_tray_icon_and_menu(app_handle, &engine_for_tray);

                            let app_clone = app_handle.clone();
                            let engine_clone = engine_for_tray.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_secs(15 * 60 + 1));
                                if !engine_clone.is_incognito_active() {
                                    let current_status = engine_clone.get_incognito_status();
                                    let _ = app_clone.emit("incognito-changed", &current_status);
                                    sync_tray_icon_and_menu(&app_clone, &engine_clone);
                                }
                            });
                        }
                        "incognito_1h" => {
                            engine_for_tray.enter_incognito(Some(60));
                            let status = engine_for_tray.get_incognito_status();
                            let _ = app_handle.emit("incognito-changed", &status);
                            sync_tray_icon_and_menu(app_handle, &engine_for_tray);

                            let app_clone = app_handle.clone();
                            let engine_clone = engine_for_tray.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_secs(60 * 60 + 1));
                                if !engine_clone.is_incognito_active() {
                                    let current_status = engine_clone.get_incognito_status();
                                    let _ = app_clone.emit("incognito-changed", &current_status);
                                    sync_tray_icon_and_menu(&app_clone, &engine_clone);
                                }
                            });
                        }
                        "toggle_autostart" => {
                            let current = engine_for_tray.is_autostart_enabled();
                            let _ = engine_for_tray.set_autostart(!current);
                            sync_tray_icon_and_menu(app_handle, &engine_for_tray);
                        }
                        "clear_history" => {
                            let _ = engine_for_tray.clear_history();
                            let _ = app_handle.emit("data-restored", ());
                        }
                        "quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            Ok(())
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
            delete_entry,
            delete_entries,
            search_snippets,
            paste_snippet,
            get_paste_queue_status,
            toggle_paste_queue,
            clear_paste_queue,
            paste_queue_pop,
            paste_multiple_entries,
            toggle_incognito,
            get_incognito_status,
            export_backup,
            import_backup,
            is_autostart_enabled,
            set_autostart,
            clear_all_history,
            get_global_shortcut,
            set_global_shortcut,
            get_storage_info,
            open_storage_dir,
            select_backup_save_path,
            select_backup_open_path,
            execute_in_place_update,
            download_and_install_update,
            get_history_capacity,
            set_history_capacity
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 展现 660×520 剪贴板管理主面板 (居中常规前台焦点模式)
pub fn show_full_window(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.set_size(tauri::PhysicalSize::new(
            crate::pal::anchor::FULL_WINDOW_WIDTH as u32,
            crate::pal::anchor::FULL_WINDOW_HEIGHT as u32,
        ));
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app_handle.emit("panel-shown", ());
    }
}

/// 切换主程序悬浮面板显示/隐藏状态
///
/// 统一采用光标/鼠标指针自适应吸附与四向翻转防溢出贴靠 (WindowAnchor & Flip-fit)
pub fn toggle_main_window(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        if let Ok(is_visible) = window.is_visible() {
            if is_visible {
                crate::commands::clipboard::safe_hide_main_window(app_handle);
            } else {
                #[cfg(windows)]
                crate::pal::windows::WindowsPlatformDriver::capture_foreground_window();

                const WIN_WIDTH: i32 = crate::pal::anchor::FULL_WINDOW_WIDTH;
                const WIN_HEIGHT: i32 = crate::pal::anchor::FULL_WINDOW_HEIGHT;

                #[cfg(windows)]
                let (x, y) = crate::pal::windows::WindowsPlatformDriver::get_window_anchor_position(
                    WIN_WIDTH,
                    WIN_HEIGHT,
                );
                #[cfg(not(windows))]
                let (x, y) = {
                    if let Ok(Some(monitor)) = window.current_monitor() {
                        let monitor_size = monitor.size();
                        let monitor_pos = monitor.position();
                        let px = monitor_pos.x + ((monitor_size.width as i32 - WIN_WIDTH) / 2);
                        let py = monitor_pos.y + ((monitor_size.height as i32 - WIN_HEIGHT) / 2);
                        (px, py)
                    } else {
                        (100, 100)
                    }
                };

                let _ = window.set_size(tauri::PhysicalSize::new(WIN_WIDTH as u32, WIN_HEIGHT as u32));
                let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                let _ = window.show();
                let _ = window.set_focus();
                let _ = app_handle.emit("panel-shown", ());
            }
        }
    }
}

/// 动态注册或更新全局主面板呼出快捷键
pub fn update_main_shortcut(
    app_handle: &tauri::AppHandle,
    new_shortcut_str: &str,
    old_shortcut_str: Option<&str>,
) -> Result<(), String> {
    let new_shortcut = Shortcut::from_str(new_shortcut_str)
        .map_err(|e| format!("快捷键格式无效: {e}"))?;

    // 1. 若指定旧快捷键，先尝试注销
    if let Some(old_str) = old_shortcut_str {
        if let Ok(old_sc) = Shortcut::from_str(old_str) {
            let _ = app_handle.global_shortcut().unregister(old_sc);
        }
    }

    // 2. 绑定新快捷键事件监听
    let app_handle_clone = app_handle.clone();
    app_handle
        .global_shortcut()
        .on_shortcut(new_shortcut, move |_app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_main_window(&app_handle_clone);
            }
        })
        .map_err(|e| format!("注册快捷键失败，可能已被系统或其他程序占用: {e}"))?;

    Ok(())
}
