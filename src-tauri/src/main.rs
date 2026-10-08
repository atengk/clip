// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Clip 应用程序可执行二进制入口点。
//!
//! 在 Windows 环境下通过具名互斥体实现单实例互斥守护 (Single Instance Guard)，
//! 当检测到已有实例在后台运行时，新进程自动唤醒既有前台主窗口并静默自退出。
//!
//! @author Ateng
//! @since 2026-10-08

#[cfg(windows)]
fn acquire_single_instance_or_focus() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\Clip_App_Single_Instance_Mutex"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // 已存在正在运行的 Clip 实例，查找已有主窗口并将其唤醒至前台
            if let Ok(hwnd) = FindWindowW(None, w!("Clip")) {
                if !hwnd.is_invalid() && hwnd.0 != std::ptr::null_mut() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            if let Ok(handle) = mutex {
                let _ = CloseHandle(handle);
            }
            return false;
        }
        // 保持 Mutex 句柄在进程存活期间有效
        std::mem::forget(mutex);
    }
    true
}

fn main() {
    #[cfg(windows)]
    {
        if !acquire_single_instance_or_focus() {
            return;
        }
    }

    clip_lib::run()
}
