//! Windows 原生平台驱动 (Win32 实现)，通过 Win32 API 提供剪贴板监听、读写与模拟输入。
//!
//! @author Ateng
//! @since 2026-10-06

use crate::pal::anchor::{
    calculate_flip_fit_position, AnchorPoint, ScreenRect, DEFAULT_ANCHOR_MARGIN,
};
use crate::pal::{PalError, PlatformDriver};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V, VK_F2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetMessageW,
    GetWindowThreadProcessId, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_DESTROY,
    WNDCLASSW, GUITHREADINFO,
};

/// 标准 Unicode 纯文本剪贴板格式 ID (CF_UNICODETEXT = 13)
const CF_UNICODETEXT: u32 = 13;

/// 标准设备无关位图剪贴板格式 ID (CF_DIB = 8)
const CF_DIB: u32 = 8;

/// 线程安全的全局监听回调包装
static GLOBAL_CALLBACK: RwLock<Option<Arc<dyn Fn() + Send + Sync>>> = RwLock::new(None);

/// 记录激活悬浮窗前的系统原活动前台窗口句柄 (HWND)
static PREVIOUS_FOREGROUND_WINDOW: RwLock<Option<isize>> = RwLock::new(None);

/// 记录激活悬浮窗前是否处于 Windows 资源管理器/桌面的就地重命名状态
static IS_EXPLORER_RENAMING: RwLock<bool> = RwLock::new(false);

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

    /// 记录当前系统活动的前台窗口句柄与重命名态（在唤起悬浮面板前调用）
    pub fn capture_foreground_window() {
        let hwnd = unsafe { GetForegroundWindow() };
        if !hwnd.0.is_null() {
            let mut guard = PREVIOUS_FOREGROUND_WINDOW.write().unwrap();
            *guard = Some(hwnd.0 as isize);

            // 探测前台是否处于资源管理器或桌面重命名态 (In-place Renaming)
            let mut is_renaming = false;
            unsafe {
                let mut class_buf = [0u16; 64];
                let len = GetClassNameW(hwnd, &mut class_buf);
                if len > 0 {
                    let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
                    if class_name == "CabinetWClass"
                        || class_name == "ExploreWClass"
                        || class_name == "Progman"
                        || class_name == "WorkerW"
                    {
                        let thread_id = GetWindowThreadProcessId(hwnd, None);
                        let mut gui_info = GUITHREADINFO {
                            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                            ..Default::default()
                        };
                        if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok()
                            && !gui_info.hwndFocus.0.is_null()
                        {
                            let mut focus_buf = [0u16; 64];
                            let focus_len = GetClassNameW(gui_info.hwndFocus, &mut focus_buf);
                            if focus_len > 0 {
                                let focus_class = String::from_utf16_lossy(&focus_buf[..focus_len as usize]);
                                if focus_class.eq_ignore_ascii_case("Edit") {
                                    is_renaming = true;
                                }
                            }
                        }
                    }
                }
            }
            let mut rename_guard = IS_EXPLORER_RENAMING.write().unwrap();
            *rename_guard = is_renaming;
        }
    }

    /// 获取当前前台是否捕获到处于资源管理器重命名态
    pub fn is_explorer_renaming() -> bool {
        let guard = IS_EXPLORER_RENAMING.read().unwrap();
        *guard
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

/// 计算光标吸附与四向翻转展示坐标 (WindowAnchor & Flip-fit Anchor)
pub fn get_window_anchor_position(win_width: i32, win_height: i32) -> (i32, i32) {
    unsafe {
        // 1. 尝试获取活动前台窗口的输入光标 (Caret)
        let foreground_hwnd = GetForegroundWindow();
        let thread_id = GetWindowThreadProcessId(foreground_hwnd, None);
        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };

        let mut anchor = None;
        if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok() && !gui_info.hwndCaret.0.is_null() {
            let mut pt = POINT {
                x: gui_info.rcCaret.left,
                y: gui_info.rcCaret.bottom,
            };
            if ClientToScreen(gui_info.hwndCaret, &mut pt).as_bool() {
                if pt.x != 0 || pt.y != 0 {
                    anchor = Some(AnchorPoint::new(pt.x, pt.y));
                }
            }
        }

        // 2. 若无有效输入光标，回退获取鼠标指针位置 (Mouse Pointer)
        let anchor_point = match anchor {
            Some(p) => p,
            None => {
                let mut cursor_pt = POINT { x: 0, y: 0 };
                if GetCursorPos(&mut cursor_pt).is_ok() {
                    AnchorPoint::new(cursor_pt.x, cursor_pt.y)
                } else {
                    AnchorPoint::new(100, 100)
                }
            }
        };

        // 3. 获取锚点所在的显示器工作区 (避让任务栏)
        let pt = POINT {
            x: anchor_point.x,
            y: anchor_point.y,
        };
        let hmonitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mon_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        let work_area = if GetMonitorInfoW(hmonitor, &mut mon_info).as_bool() {
            ScreenRect::new(
                mon_info.rcWork.left,
                mon_info.rcWork.top,
                mon_info.rcWork.right,
                mon_info.rcWork.bottom,
            )
        } else {
            ScreenRect::new(0, 0, 1920, 1040)
        };

        // 4. 执行四向翻转贴靠算法 (Flip-fit Anchor)
        calculate_flip_fit_position(
            anchor_point,
            win_width,
            win_height,
            work_area,
            DEFAULT_ANCHOR_MARGIN,
        )
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
            thread::sleep(std::time::Duration::from_millis(35));

            let was_renaming = {
                let mut guard = IS_EXPLORER_RENAMING.write().unwrap();
                let val = *guard;
                *guard = false; // 消费后及时重置状态
                val
            };

            // 构造按键模拟辅助闭包
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

            // 2. 若此前捕获到就地重命名态，执行 F2 重激活补偿 (使文件重新进入重命名编辑态并全选)
            if was_renaming {
                let f2_inputs = [
                    make_key_input(VK_F2, false),
                    make_key_input(VK_F2, true),
                ];
                let _ = SendInput(&f2_inputs, std::mem::size_of::<INPUT>() as i32);
                thread::sleep(std::time::Duration::from_millis(25));
            }

            // 3. 构造 Ctrl + V 模拟按键序列 (Down Ctrl, Down V, Up V, Up Ctrl)
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

    fn is_clipboard_ignored(&self) -> Result<bool, PalError> {
        unsafe {
            // 1. 存在即丢弃格式检测 (Clipboard Viewer Ignore & ExcludeClipboardContentFromMonitorProcessing)
            let ignore_format = RegisterClipboardFormatW(windows::core::w!("Clipboard Viewer Ignore"));
            let exclude_monitor = RegisterClipboardFormatW(windows::core::w!("ExcludeClipboardContentFromMonitorProcessing"));

            if ignore_format != 0 && IsClipboardFormatAvailable(ignore_format).is_ok() {
                return Ok(true);
            }
            if exclude_monitor != 0 && IsClipboardFormatAvailable(exclude_monitor).is_ok() {
                return Ok(true);
            }

            // 2. Windows 剪贴板历史协议 CanIncludeInClipboardHistory: DWORD 为 0 时排除，为 1 时允许
            let history_format = RegisterClipboardFormatW(windows::core::w!("CanIncludeInClipboardHistory"));
            if history_format != 0
                && IsClipboardFormatAvailable(history_format).is_ok()
                && open_clipboard_with_retry().is_ok()
            {
                let mut should_ignore = false;
                if let Ok(handle) = GetClipboardData(history_format) {
                    if !handle.0.is_null() {
                        let ptr = GlobalLock(HGLOBAL(handle.0));
                        if !ptr.is_null() {
                            let val = *(ptr as *const u32);
                            if val == 0 {
                                should_ignore = true;
                            }
                            let _ = GlobalUnlock(HGLOBAL(handle.0));
                        }
                    }
                }
                let _ = CloseClipboard();
                if should_ignore {
                    return Ok(true);
                }
            }

            Ok(false)
        }
    }

    fn get_clipboard_source_process(&self) -> Result<Option<String>, PalError> {
        unsafe {
            let foreground = GetForegroundWindow();
            if foreground.0.is_null() {
                return Ok(None);
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(foreground, Some(&mut pid));
            if pid == 0 {
                return Ok(None);
            }
            let process_handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(h) => h,
                Err(_) => return Ok(None),
            };

            let mut buffer = [0u16; 1024];
            let mut size = buffer.len() as u32;
            let query_res = QueryFullProcessImageNameW(
                process_handle,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(process_handle);

            if query_res.is_ok() && size > 0 {
                let full_path = String::from_utf16_lossy(&buffer[..size as usize]);
                let file_name = std::path::Path::new(&full_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string());
                return Ok(file_name);
            }

            Ok(None)
        }
    }

    fn read_image(&self) -> Result<Option<Vec<u8>>, PalError> {
        unsafe {
            // 1. 安全打开剪贴板
            open_clipboard_with_retry()?;

            // 2. 检查 CF_DIB 格式是否就绪
            if IsClipboardFormatAvailable(CF_DIB).is_err() {
                let _ = CloseClipboard();
                return Ok(None);
            }

            // 3. 获取位图句柄并解析 DIB 数据
            let handle = match GetClipboardData(CF_DIB) {
                Ok(h) if !h.0.is_null() => h,
                _ => {
                    let _ = CloseClipboard();
                    return Ok(None);
                }
            };

            let hglobal = HGLOBAL(handle.0);
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                let _ = CloseClipboard();
                return Err(PalError::ClipboardError("锁定剪贴板图片内存块失败".into()));
            }

            let dib_size = windows::Win32::System::Memory::GlobalSize(hglobal);
            if dib_size < 40 {
                let _ = GlobalUnlock(hglobal);
                let _ = CloseClipboard();
                return Ok(None);
            }

            let dib_slice = std::slice::from_raw_parts(ptr as *const u8, dib_size);

            // 4. 解析 DIB 信息头计算颜色表偏移
            let bi_size = u32::from_le_bytes(dib_slice[0..4].try_into().unwrap_or_default());
            let bi_bit_count = u16::from_le_bytes(dib_slice[14..16].try_into().unwrap_or_default());
            let bi_compression = u32::from_le_bytes(dib_slice[16..20].try_into().unwrap_or_default());
            let bi_clr_used = u32::from_le_bytes(dib_slice[32..36].try_into().unwrap_or_default());

            let colors = if bi_clr_used != 0 {
                bi_clr_used
            } else if bi_bit_count <= 8 {
                1 << bi_bit_count
            } else {
                0
            };
            let masks_size = if bi_compression == 3 && bi_size == 40 { 12 } else { 0 };
            let offset = 14 + bi_size + (colors * 4) + masks_size;

            // 5. 组装标准 14 字节 BITMAPFILEHEADER
            let mut bmp_bytes = Vec::with_capacity(14 + dib_size);
            bmp_bytes.extend_from_slice(b"BM");
            let total_size = (14 + dib_size) as u32;
            bmp_bytes.extend_from_slice(&total_size.to_le_bytes());
            bmp_bytes.extend_from_slice(&[0u8; 4]); // 保留字段
            bmp_bytes.extend_from_slice(&offset.to_le_bytes());
            bmp_bytes.extend_from_slice(dib_slice);

            let _ = GlobalUnlock(hglobal);
            let _ = CloseClipboard();

            Ok(Some(bmp_bytes))
        }
    }

    fn write_image(&self, data: &[u8]) -> Result<(), PalError> {
        if data.is_empty() {
            return Ok(());
        }

        // 若传入包含 14 字节 BMP 头，剥离为原生 DIB 字节流以适配 Win32 CF_DIB 规范
        let dib_payload = if data.len() >= 14 && &data[0..2] == b"BM" {
            &data[14..]
        } else {
            data
        };

        unsafe {
            // 1. 分配可移动全局内存
            let hglobal = match GlobalAlloc(GMEM_MOVEABLE, dib_payload.len()) {
                Ok(h) if !h.0.is_null() => h,
                _ => return Err(PalError::ClipboardError("分配剪贴板位图内存失败".into())),
            };

            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                return Err(PalError::ClipboardError("锁定剪贴板位图内存失败".into()));
            }

            std::ptr::copy_nonoverlapping(dib_payload.as_ptr(), ptr as *mut u8, dib_payload.len());
            let _ = GlobalUnlock(hglobal);

            // 2. 打开并写入剪贴板
            open_clipboard_with_retry()?;

            if let Err(e) = EmptyClipboard() {
                let _ = CloseClipboard();
                return Err(PalError::ClipboardError(format!("清空剪贴板失败: {e}")));
            }

            if let Err(e) = SetClipboardData(CF_DIB, HANDLE(hglobal.0)) {
                let _ = CloseClipboard();
                return Err(PalError::ClipboardError(format!("写入剪贴板位图失败: {e}")));
            }

            let _ = CloseClipboard();
            Ok(())
        }
    }

    fn ocr_image(&self, data: &[u8]) -> Result<String, PalError> {
        if data.is_empty() {
            return Ok(String::new());
        }

        use windows::Graphics::Imaging::BitmapDecoder;
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

        let run_ocr = || -> windows::core::Result<String> {
            // 1. 尝试初始化当前用户语言环境的 OCR 引擎
            let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;

            // 2. 写入内存数据流并解码为 SoftwareBitmap
            let stream = InMemoryRandomAccessStream::new()?;
            let writer = DataWriter::CreateDataWriter(&stream)?;
            writer.WriteBytes(data)?;
            writer.StoreAsync()?.get()?;
            writer.FlushAsync()?.get()?;
            stream.Seek(0)?;

            let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
            let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;

            // 3. 执行原生离线文字识别并获取完整文本
            let ocr_result = engine.RecognizeAsync(&bitmap)?.get()?;
            let recognized_text = ocr_result.Text()?.to_string();

            Ok(recognized_text)
        };

        match run_ocr() {
            Ok(text) => Ok(text),
            Err(e) => Err(PalError::InternalError(format!("Windows 原生 OCR 识别失败: {e}"))),
        }
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
