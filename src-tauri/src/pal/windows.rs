//! Windows 原生平台驱动 (Win32 实现)，通过 Win32 API 提供剪贴板监听、读写与模拟输入。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::pal::{PalError, PlatformDriver};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard,
    RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_DESTROY,
    WNDCLASSW,
};

/// 标准 Unicode 纯文本剪贴板格式 ID (CF_UNICODETEXT = 13)
const CF_UNICODETEXT: u32 = 13;

/// 线程安全的全局监听回调包装
static GLOBAL_CALLBACK: RwLock<Option<Arc<dyn Fn() + Send + Sync>>> = RwLock::new(None);

/// 记录激活悬浮窗前的系统原活动前台窗口句柄 (HWND)
static PREVIOUS_FOREGROUND_WINDOW: RwLock<Option<isize>> = RwLock::new(None);

/// 带重试机制安全打开 Win32 剪贴板
fn open_clipboard_with_retry() -> Result<(), PalError> {
    for _ in 0..5 {
        if unsafe { OpenClipboard(HWND(std::ptr::null_mut())).is_ok() } {
            return Ok(());
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    Err(PalError::ClipboardError("无法获取系统剪贴板访问锁".into()))
}

/// Windows 原生平台驱动
pub struct WindowsPlatformDriver {
    is_monitoring: Arc<AtomicBool>,
}

impl WindowsPlatformDriver {
    /// 创建 Windows 原生平台驱动实例
    pub fn new() -> Self {
        Self {
            is_monitoring: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 记录当前系统活动的前台窗口句柄（在唤起悬浮面板前调用）
    pub fn capture_foreground_window() {
        let hwnd = unsafe { GetForegroundWindow() };
        if !hwnd.0.is_null() {
            let mut guard = PREVIOUS_FOREGROUND_WINDOW.write().unwrap();
            *guard = Some(hwnd.0 as isize);
        }
    }

    /// 恢复上一个活动窗口的前台焦点状态
    pub fn restore_foreground_window() {
        let prev_opt = {
            let guard = PREVIOUS_FOREGROUND_WINDOW.read().unwrap();
            *guard
        };
        if let Some(raw_hwnd) = prev_opt {
            unsafe {
                let hwnd = HWND(raw_hwnd as *mut _);
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

impl Default for WindowsPlatformDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformDriver for WindowsPlatformDriver {
    fn read_text(&self) -> Result<Option<String>, PalError> {
        unsafe {
            // 1. 安全打开剪贴板
            open_clipboard_with_retry()?;

            // 2. 获取 CF_UNICODETEXT 数据句柄
            let handle_res = GetClipboardData(CF_UNICODETEXT);
            let handle = match handle_res {
                Ok(h) => h,
                Err(_) => {
                    let _ = CloseClipboard();
                    return Ok(None);
                }
            };

            if handle.0.is_null() {
                let _ = CloseClipboard();
                return Ok(None);
            }

            // 3. 锁定内存读取 UTF-16 数据
            let ptr = GlobalLock(HGLOBAL(handle.0)) as *const u16;
            if ptr.is_null() {
                let _ = CloseClipboard();
                return Err(PalError::ClipboardError("锁定剪贴板内存失败".into()));
            }

            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);

            let _ = GlobalUnlock(HGLOBAL(handle.0));
            let _ = CloseClipboard();

            Ok(Some(text))
        }
    }

    fn write_text(&self, text: &str) -> Result<(), PalError> {
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes_len = utf16.len() * std::mem::size_of::<u16>();

        unsafe {
            // 1. 分配可移动内存
            let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes_len)
                .map_err(|e| PalError::ClipboardError(format!("分配剪贴板全局内存失败: {e}")))?;

            let ptr = GlobalLock(hglobal) as *mut u16;
            if ptr.is_null() {
                return Err(PalError::ClipboardError("锁定待写入内存失败".into()));
            }
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
            let _ = GlobalUnlock(hglobal);

            // 2. 打开并置空剪贴板写入新内容
            open_clipboard_with_retry()?;

            if let Err(e) = EmptyClipboard() {
                let _ = CloseClipboard();
                return Err(PalError::ClipboardError(format!("清空剪贴板失败: {e}")));
            }

            if let Err(e) = SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0)) {
                let _ = CloseClipboard();
                return Err(PalError::ClipboardError(format!("写入剪贴板数据失败: {e}")));
            }

            let _ = CloseClipboard();
            Ok(())
        }
    }

    fn send_paste(&self) -> Result<(), PalError> {
        unsafe {
            // 1. 显式还原原前台窗口焦点
            Self::restore_foreground_window();
            thread::sleep(std::time::Duration::from_millis(20));

            // 2. 构造 Ctrl + V 模拟按键序列 (Down Ctrl, Down V, Up V, Up Ctrl)
            let make_key_input = |vk: VIRTUAL_KEY, is_up: bool| -> INPUT {
                let mut flags = KEYBD_EVENT_FLAGS(0);
                if is_up {
                    flags |= KEYEVENTF_KEYUP;
                }
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: vk,
                            wScan: 0,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                }
            };

            let inputs = [
                make_key_input(VK_CONTROL, false),
                make_key_input(VK_V, false),
                make_key_input(VK_V, true),
                make_key_input(VK_CONTROL, true),
            ];

            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent != inputs.len() as u32 {
                return Err(PalError::InputSimulationError(
                    "发送模拟按键输入失败".into(),
                ));
            }
            Ok(())
        }
    }

    fn start_monitor(&self, callback: Arc<dyn Fn() + Send + Sync>) -> Result<(), PalError> {
        if self.is_monitoring.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let is_monitoring = self.is_monitoring.clone();
        thread::Builder::new()
            .name("clip-win32-clipboard-monitor".into())
            .spawn(move || unsafe {
                let class_name = windows::core::w!("ClipClipboardListenerWindow");
                let wnd_class = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(listener_wndproc),
                    hInstance: windows::Win32::Foundation::HINSTANCE(std::ptr::null_mut()),
                    lpszClassName: class_name,
                    ..Default::default()
                };

                let _ = RegisterClassW(&wnd_class);

                // 创建纯消息隐藏窗口
                let hwnd = match CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class_name,
                    windows::core::w!("ClipListener"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    HWND(std::ptr::null_mut()),
                    None,
                    windows::Win32::Foundation::HINSTANCE(std::ptr::null_mut()),
                    None,
                ) {
                    Ok(h) => h,
                    Err(_) => {
                        is_monitoring.store(false, Ordering::SeqCst);
                        return;
                    }
                };

                // 注册剪贴板格式监听器
                if AddClipboardFormatListener(hwnd).is_err() {
                    let _ = DestroyWindow(hwnd);
                    is_monitoring.store(false, Ordering::SeqCst);
                    return;
                }

                // 安全存储全局回调
                {
                    let mut guard = GLOBAL_CALLBACK.write().unwrap();
                    *guard = Some(callback);
                }

                let mut msg = MSG::default();
                while is_monitoring.load(Ordering::SeqCst)
                    && GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool()
                {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let _ = RemoveClipboardFormatListener(hwnd);
                let _ = DestroyWindow(hwnd);
                {
                    let mut guard = GLOBAL_CALLBACK.write().unwrap();
                    *guard = None;
                }
            })
            .map_err(|e| PalError::MonitorError(format!("创建监听线程失败: {e}")))?;

        Ok(())
    }
}

unsafe extern "system" fn listener_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            let cb_opt = {
                let guard = GLOBAL_CALLBACK.read().unwrap();
                guard.clone()
            };
            if let Some(cb) = cb_opt {
                cb();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
