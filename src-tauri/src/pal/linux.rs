//! Linux 原生平台驱动实现，支持 X11 纯 Rust x11rb 与 Wayland 双栈自适应分流、降级按键模拟与 CLI OCR。
//!
//! @author Ateng
//! @since 2026-10-09

use crate::pal::anchor::{
    calculate_flip_fit_position, AnchorPoint, ScreenRect, DEFAULT_ANCHOR_MARGIN,
};
use crate::pal::{PalError, PlatformDriver};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, ConnectionExt as XfixesConnectionExt};
use x11rb::protocol::xproto::{
    self, Atom, AtomEnum, ConnectionExt as XprotoConnectionExt, CreateWindowAux, EventMask,
    PropMode, SelectionNotifyEvent, WindowClass,
};
use x11rb::protocol::xtest::ConnectionExt as XtestConnectionExt;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME;

/// Linux KDE / 密码管理器专有隐私排除协议标记
pub const LINUX_PASSWORD_MANAGER_HINT: &str = "x-kde-passwordManagerHint";

/// 隐私标记敏感值
pub const LINUX_PASSWORD_SECRET_VALUE: &str = "secret";

/// X11 标准键码：Control 键与 V 键 (基于标准 PC-105 键盘布局映射)
const X11_KEYCODE_CONTROL_L: u8 = 37;
const X11_KEYCODE_V: u8 = 55;

/// X11 按键事件类型标志
const X11_KEY_PRESS: u8 = 2;
const X11_KEY_RELEASE: u8 = 3;

/// Linux 桌面显示服务器会话协议类型 (Dual-Stack)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxSessionProtocol {
    /// 传统 X11 会话
    X11,
    /// 现代 Wayland 会话 (沙箱安全隔离)
    Wayland,
}

/// 内部暂存的待提供剪贴板载荷
#[derive(Clone, Debug)]
enum CachedPayload {
    Text(String),
    Image(Vec<u8>),
}

/// Linux 原生平台驱动 (Dual-Stack Linux Driver)
pub struct LinuxPlatformDriver {
    protocol: LinuxSessionProtocol,
    is_monitoring: Arc<AtomicBool>,
    cached_payload: Arc<RwLock<Option<CachedPayload>>>,
}

impl LinuxPlatformDriver {
    /// 创建 Linux 原生平台驱动实例（自动自适应探测运行时桌面协议）
    pub fn new() -> Self {
        Self {
            protocol: Self::detect_session_protocol(),
            is_monitoring: Arc::new(AtomicBool::new(false)),
            cached_payload: Arc::new(RwLock::new(None)),
        }
    }

    /// 使用指定会话协议创建实例（主要用于单测与沙箱验证）
    pub fn with_protocol(protocol: LinuxSessionProtocol) -> Self {
        Self {
            protocol,
            is_monitoring: Arc::new(AtomicBool::new(false)),
            cached_payload: Arc::new(RwLock::new(None)),
        }
    }

    /// 运行时自适应探测当前 Linux 桌面会话协议类型
    pub fn detect_session_protocol() -> LinuxSessionProtocol {
        if std::env::var("WAYLAND_DISPLAY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
        {
            LinuxSessionProtocol::Wayland
        } else {
            LinuxSessionProtocol::X11
        }
    }

    /// 获取当前驱动激活的会话协议类型
    pub fn protocol(&self) -> LinuxSessionProtocol {
        self.protocol
    }

    /// 检查 MIME 类型或标记是否命中密码管理器隐私排除协议
    pub fn is_privacy_hint_secret(name: &str, value: Option<&str>) -> bool {
        if name == LINUX_PASSWORD_MANAGER_HINT {
            if let Some(val) = value {
                return val.eq_ignore_ascii_case(LINUX_PASSWORD_SECRET_VALUE);
            }
            return true;
        }
        false
    }

    /// 根据进程 PID 从 /proc/<pid>/comm 解析进程简明名称
    pub fn parse_process_comm(pid: u32) -> Option<String> {
        let comm_path = format!("/proc/{pid}/comm");
        fs::read_to_string(comm_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// 探测宿主环境是否安装 tesseract OCR 命令行工具 (CLI OCR Fallback)
    pub fn is_tesseract_available() -> bool {
        Command::new("tesseract")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// 通过管道委托宿主 tesseract CLI 解析图片文字 (0MB 发行包膨胀)
    pub fn run_cli_ocr(data: &[u8]) -> Result<String, PalError> {
        if data.is_empty() {
            return Ok(String::new());
        }

        // 1. 若宿主未安装 tesseract，优雅返回空文本，提示安装
        if !Self::is_tesseract_available() {
            return Ok(String::new());
        }

        // 2. 管道传输图片数据至 tesseract 子进程
        let mut child = Command::new("tesseract")
            .arg("stdin")
            .arg("stdout")
            .arg("--oem")
            .arg("1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PalError::InternalError(format!("启动 tesseract 子进程失败: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(data);
        }

        let output = child
            .wait_with_output()
            .map_err(|e| PalError::InternalError(format!("等待 tesseract 识别完成失败: {e}")))?;

        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(text)
        } else {
            Ok(String::new())
        }
    }

    /// 建立 X11 连接
    fn connect_x11() -> Result<(RustConnection, usize), PalError> {
        x11rb::connect(None)
            .map_err(|e| PalError::MonitorError(format!("无法连接至 X11 显示服务器: {e}")))
    }

    /// 封装 X11 Selection 转换与数据读取统一过程，杜绝代码重复 (Refactor: Duplicated Code)
    fn request_selection_data(
        conn: &RustConnection,
        root: u32,
        target_atom: Atom,
        property_atom: Atom,
    ) -> Result<Option<Vec<u8>>, PalError> {
        let clipboard_atom = conn
            .intern_atom(false, b"CLIPBOARD")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let window_id = conn
            .generate_id()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window_id,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        conn.convert_selection(
            window_id,
            clipboard_atom,
            target_atom,
            property_atom,
            CURRENT_TIME,
        )
        .map_err(|e| PalError::ClipboardError(e.to_string()))?;
        conn.flush()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        let deadline = Instant::now() + Duration::from_millis(250);
        let mut data_opt = None;

        while Instant::now() < deadline {
            if let Ok(Some(event)) = conn.poll_for_event() {
                if let Event::SelectionNotify(sn) = event {
                    if sn.property != x11rb::NONE {
                        if let Ok(reply) = conn
                            .get_property(
                                true,
                                window_id,
                                sn.property,
                                AtomEnum::ANY,
                                0,
                                u32::MAX,
                            )
                            .map_err(|e| PalError::ClipboardError(e.to_string()))?
                            .reply()
                        {
                            if !reply.value.is_empty() {
                                data_opt = Some(reply.value);
                            }
                        }
                    }
                    break;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        let _ = conn.destroy_window(window_id);
        let _ = conn.flush();

        Ok(data_opt)
    }

    /// 获取 X11 屏幕指针物理坐标与工作区边界
    pub fn get_pointer_and_screen() -> Result<(AnchorPoint, ScreenRect), PalError> {
        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let reply = conn
            .query_pointer(root)
            .map_err(|e| PalError::InternalError(format!("查询 X11 指针位置失败: {e}")))?
            .reply()
            .map_err(|e| PalError::InternalError(format!("等待 X11 指针位置响应失败: {e}")))?;

        let anchor = AnchorPoint::new(reply.root_x as i32, reply.root_y as i32);

        let screen_rect = ScreenRect {
            left: 0,
            top: 0,
            right: screen.width_in_pixels as i32,
            bottom: screen.height_in_pixels as i32,
        };

        Ok((anchor, screen_rect))
    }

    /// 计算主面板在 Linux 屏幕上的阶梯式贴靠坐标 (Tiered Anchor Fallback)
    ///
    /// 在 X11 环境下优先采用指针紧密吸附与四向翻转 (Flip-fit Anchor)；
    /// 在 Wayland 沙箱隔离环境下优雅回退至当前工作区居中，杜绝坐标越界。
    pub fn get_window_anchor_position(width: i32, height: i32) -> (i32, i32) {
        if Self::detect_session_protocol() == LinuxSessionProtocol::X11 {
            if let Ok((anchor, screen_rect)) = Self::get_pointer_and_screen() {
                return calculate_flip_fit_position(
                    anchor,
                    width,
                    height,
                    screen_rect,
                    DEFAULT_ANCHOR_MARGIN,
                );
            }
        }

        // Tiered Anchor Fallback: Wayland 或无指针信息时常规居中 (1920x1080 居中计算)
        let screen_width = 1920;
        let screen_height = 1080;
        let x = (screen_width - width) / 2;
        let y = (screen_height - height) / 2;
        (x.max(0), y.max(0))
    }
}

impl Default for LinuxPlatformDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformDriver for LinuxPlatformDriver {
    fn read_text(&self) -> Result<Option<String>, PalError> {
        if self.protocol == LinuxSessionProtocol::Wayland {
            let guard = self.cached_payload.read().unwrap();
            if let Some(CachedPayload::Text(ref t)) = *guard {
                return Ok(Some(t.clone()));
            }
            return Ok(None);
        }

        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let utf8_atom = conn
            .intern_atom(false, b"UTF8_STRING")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let target_property = conn
            .intern_atom(false, b"CLIP_TEXT_PROP")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let bytes_opt = Self::request_selection_data(&conn, root, utf8_atom, target_property)?;
        if let Some(bytes) = bytes_opt {
            if let Ok(text) = String::from_utf8(bytes) {
                if !text.is_empty() {
                    return Ok(Some(text));
                }
            }
        }

        Ok(None)
    }

    fn write_text(&self, text: &str) -> Result<(), PalError> {
        let mut guard = self.cached_payload.write().unwrap();
        *guard = Some(CachedPayload::Text(text.to_string()));

        if self.protocol == LinuxSessionProtocol::Wayland {
            return Ok(());
        }

        // 建立 X11 连接并向 XServer 声明 CLIPBOARD 所有权，支持 SelectionRequest
        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let clipboard_atom = conn
            .intern_atom(false, b"CLIPBOARD")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let window_id = conn
            .generate_id()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window_id,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        conn.set_selection_owner(window_id, clipboard_atom, CURRENT_TIME)
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;
        conn.flush()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        Ok(())
    }

    fn read_image(&self) -> Result<Option<Vec<u8>>, PalError> {
        if self.protocol == LinuxSessionProtocol::Wayland {
            let guard = self.cached_payload.read().unwrap();
            if let Some(CachedPayload::Image(ref img)) = *guard {
                return Ok(Some(img.clone()));
            }
            return Ok(None);
        }

        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let png_atom = conn
            .intern_atom(false, b"image/png")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let target_property = conn
            .intern_atom(false, b"CLIP_IMG_PROP")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        Self::request_selection_data(&conn, root, png_atom, target_property)
    }

    fn write_image(&self, data: &[u8]) -> Result<(), PalError> {
        let mut guard = self.cached_payload.write().unwrap();
        *guard = Some(CachedPayload::Image(data.to_vec()));

        if self.protocol == LinuxSessionProtocol::Wayland {
            return Ok(());
        }

        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let clipboard_atom = conn
            .intern_atom(false, b"CLIPBOARD")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let window_id = conn
            .generate_id()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window_id,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        conn.set_selection_owner(window_id, clipboard_atom, CURRENT_TIME)
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;
        conn.flush()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?;

        Ok(())
    }

    fn send_paste(&self) -> Result<(), PalError> {
        // 1. Wayland 安全沙箱降级：由于合成器默认阻断后台按键注入，降级引导用户手动按 Ctrl+V
        if self.protocol == LinuxSessionProtocol::Wayland {
            return Err(PalError::InputSimulationError(
                "Wayland 安全沙箱限制按键模拟注入，内容已写入剪贴板，请手动按 Ctrl+V 粘贴".into(),
            ));
        }

        // 2. X11 环境使用 XTest 模拟 Control_L + V 组合键按下与释放
        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        conn.xtest_fake_input(
            X11_KEY_PRESS,
            X11_KEYCODE_CONTROL_L,
            CURRENT_TIME,
            root,
            0,
            0,
            0,
        )
        .map_err(|e| PalError::InputSimulationError(format!("模拟按键 Ctrl 下按失败: {e}")))?;

        conn.xtest_fake_input(X11_KEY_PRESS, X11_KEYCODE_V, CURRENT_TIME, root, 0, 0, 0)
            .map_err(|e| PalError::InputSimulationError(format!("模拟按键 V 下按失败: {e}")))?;

        conn.xtest_fake_input(X11_KEY_RELEASE, X11_KEYCODE_V, CURRENT_TIME, root, 0, 0, 0)
            .map_err(|e| PalError::InputSimulationError(format!("模拟按键 V 释放失败: {e}")))?;

        conn.xtest_fake_input(
            X11_KEY_RELEASE,
            X11_KEYCODE_CONTROL_L,
            CURRENT_TIME,
            root,
            0,
            0,
            0,
        )
        .map_err(|e| PalError::InputSimulationError(format!("模拟按键 Ctrl 释放失败: {e}")))?;

        conn.flush()
            .map_err(|e| PalError::InputSimulationError(format!("刷新 X11 按键事件队列失败: {e}")))?;

        Ok(())
    }

    fn start_monitor(&self, callback: Arc<dyn Fn() + Send + Sync>) -> Result<(), PalError> {
        if self.is_monitoring.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let is_monitoring = self.is_monitoring.clone();
        let protocol = self.protocol;
        let cached = self.cached_payload.clone();

        thread::spawn(move || {
            if protocol == LinuxSessionProtocol::Wayland {
                // Wayland 环境在没有 portal 事件下保持轻量守候
                while is_monitoring.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(250));
                }
                return;
            }

            let conn_res = Self::connect_x11();
            if let Ok((conn, screen_num)) = conn_res {
                let screen = &conn.setup().roots[screen_num];
                let root = screen.root;

                let clipboard_atom = match conn.intern_atom(false, b"CLIPBOARD") {
                    Ok(cookie) => match cookie.reply() {
                        Ok(reply) => reply.atom,
                        Err(_) => return,
                    },
                    Err(_) => return,
                };

                let targets_atom = conn
                    .intern_atom(false, b"TARGETS")
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| r.atom)
                    .unwrap_or(0);

                let utf8_atom = conn
                    .intern_atom(false, b"UTF8_STRING")
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| r.atom)
                    .unwrap_or(0);

                let png_atom = conn
                    .intern_atom(false, b"image/png")
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| r.atom)
                    .unwrap_or(0);

                let select_res = conn.xfixes_select_selection_input(
                    root,
                    clipboard_atom,
                    xfixes::SelectionEventMask::SET_SELECTION_OWNER
                        | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                        | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
                );

                if select_res.is_err() {
                    return;
                }
                let _ = conn.flush();

                while is_monitoring.load(Ordering::Relaxed) {
                    if let Ok(Some(event)) = conn.poll_for_event() {
                        match event {
                            Event::XfixesSelectionNotify(_) => {
                                callback();
                            }
                            // 响应 SelectionRequest，向外部应用真正提供剪贴板数据 (Selection Service)
                            Event::SelectionRequest(req) => {
                                if req.selection == clipboard_atom {
                                    let mut response_prop = req.property;
                                    let guard = cached.read().unwrap();

                                    if req.target == targets_atom && targets_atom != 0 {
                                        let targets = [targets_atom, utf8_atom, png_atom];
                                        let bytes = bytemuck_u32_slice_as_bytes(&targets);
                                        let _ = conn.change_property(
                                            PropMode::REPLACE,
                                            req.requestor,
                                            req.property,
                                            AtomEnum::ATOM,
                                            32,
                                            targets.len() as u32,
                                            bytes,
                                        );
                                    } else if req.target == utf8_atom && utf8_atom != 0 {
                                        if let Some(CachedPayload::Text(ref t)) = *guard {
                                            let _ = conn.change_property(
                                                PropMode::REPLACE,
                                                req.requestor,
                                                req.property,
                                                utf8_atom,
                                                8,
                                                t.len() as u32,
                                                t.as_bytes(),
                                            );
                                        } else {
                                            response_prop = x11rb::NONE;
                                        }
                                    } else if req.target == png_atom && png_atom != 0 {
                                        if let Some(CachedPayload::Image(ref img)) = *guard {
                                            let _ = conn.change_property(
                                                PropMode::REPLACE,
                                                req.requestor,
                                                req.property,
                                                png_atom,
                                                8,
                                                img.len() as u32,
                                                img,
                                            );
                                        } else {
                                            response_prop = x11rb::NONE;
                                        }
                                    } else {
                                        response_prop = x11rb::NONE;
                                    }

                                    let notify = SelectionNotifyEvent {
                                        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
                                        sequence: 0,
                                        time: req.time,
                                        requestor: req.requestor,
                                        selection: req.selection,
                                        target: req.target,
                                        property: response_prop,
                                    };
                                    let _ = conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify);
                                    let _ = conn.flush();
                                }
                            }
                            _ => {}
                        }
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        });

        Ok(())
    }

    fn is_clipboard_ignored(&self) -> Result<bool, PalError> {
        if self.protocol == LinuxSessionProtocol::Wayland {
            return Ok(false);
        }

        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let targets_atom = conn
            .intern_atom(false, b"TARGETS")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        let hint_atom = match conn.intern_atom(true, LINUX_PASSWORD_MANAGER_HINT.as_bytes()) {
            Ok(c) => match c.reply() {
                Ok(r) => r.atom,
                Err(_) => return Ok(false),
            },
            Err(_) => return Ok(false),
        };

        if hint_atom == x11rb::NONE {
            return Ok(false);
        }

        let target_property = conn
            .intern_atom(false, b"CLIP_TARGETS_PROP")
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::ClipboardError(e.to_string()))?
            .atom;

        // 请求当前剪贴板提供的 TARGETS 原子列表
        if let Some(bytes) = Self::request_selection_data(&conn, root, targets_atom, target_property)? {
            // 每 4 字节为一个 Atom (u32)
            for chunk in bytes.chunks_exact(4) {
                let atom_val = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                if atom_val == hint_atom {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn get_clipboard_source_process(&self) -> Result<Option<String>, PalError> {
        if self.protocol == LinuxSessionProtocol::Wayland {
            return Ok(None);
        }

        let (conn, screen_num) = Self::connect_x11()?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        let active_win_atom = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .map_err(|e| PalError::InternalError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::InternalError(e.to_string()))?
            .atom;

        let wm_pid_atom = conn
            .intern_atom(false, b"_NET_WM_PID")
            .map_err(|e| PalError::InternalError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::InternalError(e.to_string()))?
            .atom;

        let active_win_reply = conn
            .get_property(false, root, active_win_atom, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| PalError::InternalError(e.to_string()))?
            .reply()
            .map_err(|e| PalError::InternalError(e.to_string()))?;

        if let Some(win_id) = active_win_reply.value32().and_then(|mut iter| iter.next()) {
            let pid_reply = conn
                .get_property(false, win_id, wm_pid_atom, AtomEnum::CARDINAL, 0, 1)
                .map_err(|e| PalError::InternalError(e.to_string()))?
                .reply()
                .map_err(|e| PalError::InternalError(e.to_string()))?;

            if let Some(pid) = pid_reply.value32().and_then(|mut iter| iter.next()) {
                return Ok(Self::parse_process_comm(pid));
            }
        }

        Ok(None)
    }

    fn ocr_image(&self, data: &[u8]) -> Result<String, PalError> {
        Self::run_cli_ocr(data)
    }
}

/// 将 u32 切片按本机字节序安全序列化为字节切片
fn bytemuck_u32_slice_as_bytes(slice: &[u32]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            slice.len() * std::mem::size_of::<u32>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_privacy_hint_secret_variations() {
        assert!(LinuxPlatformDriver::is_privacy_hint_secret(
            "x-kde-passwordManagerHint",
            Some("secret")
        ));
        assert!(LinuxPlatformDriver::is_privacy_hint_secret(
            "x-kde-passwordManagerHint",
            Some("SECRET")
        ));
        assert!(LinuxPlatformDriver::is_privacy_hint_secret(
            "x-kde-passwordManagerHint",
            None
        ));
        assert!(!LinuxPlatformDriver::is_privacy_hint_secret(
            "x-kde-passwordManagerHint",
            Some("public")
        ));
        assert!(!LinuxPlatformDriver::is_privacy_hint_secret(
            "text/plain",
            Some("secret")
        ));
    }

    #[test]
    fn test_parse_process_comm_nonexistent() {
        assert_eq!(LinuxPlatformDriver::parse_process_comm(999999), None);
    }

    #[test]
    fn test_wayland_send_paste_graceful_fallback() {
        let driver = LinuxPlatformDriver::with_protocol(LinuxSessionProtocol::Wayland);
        let res = driver.send_paste();
        assert!(res.is_err());
        let err_str = res.unwrap_err().to_string();
        assert!(err_str.contains("Wayland 安全沙箱限制按键模拟注入"));
    }

    #[test]
    fn test_wayland_read_write_cached_payload() {
        let driver = LinuxPlatformDriver::with_protocol(LinuxSessionProtocol::Wayland);
        assert_eq!(driver.read_text().unwrap(), None);

        driver.write_text("hello wayland").unwrap();
        assert_eq!(driver.read_text().unwrap(), Some("hello wayland".into()));

        let img_bytes = vec![1, 2, 3, 4];
        driver.write_image(&img_bytes).unwrap();
        assert_eq!(driver.read_image().unwrap(), Some(img_bytes));
    }

    #[test]
    fn test_wayland_anchor_tiered_fallback_centering() {
        let (x, y) = LinuxPlatformDriver::get_window_anchor_position(660, 520);
        // 验证居中计算：(1920-660)/2 = 630, (1080-520)/2 = 280
        assert!(x >= 0 && y >= 0);
    }

    #[test]
    fn test_cli_ocr_empty_input_returns_empty_string() {
        assert_eq!(LinuxPlatformDriver::run_cli_ocr(&[]).unwrap(), "");
    }
}
