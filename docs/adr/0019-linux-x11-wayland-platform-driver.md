---
status: accepted
date: 2026-10-09
---

# Linux X11 与 Wayland 双栈平台驱动架构决策 (Linux Dual-Stack Platform Driver)

## 背景与问题 (Context)

Linux 桌面环境当前面临 X11 与 Wayland 两套显示服务器协议并存的碎片化生态：
1. **协议模型差异显著**：X11 协议支持全局按键模拟与事件订阅；现代 Wayland 协议出于安全沙箱隔离，默认限制后台程序任意截获或伪造输入；
2. **构建卫生与 C 库依赖**：若依赖复杂底层 C 共享库（如 `libx11-dev`、`libxtst-dev`），极易在不同发行版（Debian/Fedora/Arch）或 CI 交叉编译环境中导致链接失败；
3. **系统级 OCR 运行时缺失**：Linux 无内置类似 Win32/macOS 的统一系统级 OCR API。

## 方案选型与决策 (Decision)

经 `/grill-with-docs` 架构质询推演，确定采用以下双栈与降级技术架构：

### 1. 运行时双栈自适应分流 (Dual-Stack Linux Driver)
- **协议探测**：通过环境变量 `WAYLAND_DISPLAY` 与 `DISPLAY` 在运行时自适应确定当前桌面会话类型；
- **纯 Rust X11 驱动**：引入纯 Rust 实现的 `x11rb`，无需本地安装系统 C 开发库，在 X11 环境下：
  - 使用 `XFixesSelectSelectionInput` 订阅 `CLIPBOARD` 原子更新，纯事件驱动，零轮询开销；
  - 使用 `XTestFakeKeyEvent` 执行零提权按键模拟回填（`Ctrl + V`）；
  - 使用 `XQueryPointer` 查询指针物理位置，执行 `Flip-fit Anchor` 光标贴靠；
  - 嗅探 `_NET_ACTIVE_WINDOW` 与 `/proc/<pid>/comm` 获取前台进程名；
- **Wayland 适配与安全沙箱降级**：
  - 剪贴板交互对接 `wlr-data-control` 协议或标准 Portal 数据源；
  - 当全局按键注入受沙箱拦截时，执行平滑降级：将目标条目写入系统剪贴板，并通过前端通知提示用户手动执行 `Ctrl + V`；
  - 坐标定位执行 **Tiered Anchor Fallback**：优雅回退至当前活动屏幕居中位置呈现。

### 2. 隐私隔离标记检测
- 检测剪贴板 MIME 元数据中是否包含 `x-kde-passwordManagerHint`（值为 `secret`）；若匹配则立即视为敏感数据予以拦截，杜绝 KeePassXC / 1Password 密码落盘。

### 3. 动态探测离线 OCR 降级链路 (CLI OCR Fallback)
- **检测机制**：运行时使用低开销命令探针检测系统 `PATH` 是否包含 `tesseract` 可执行文件；
- **管道调度**：若已安装，通过子进程以管道方式传入图片并捕获标准输出解析文本；
- **体积与可用性**：若未安装，优雅返回空文本并提示可按需安装 `tesseract-ocr`，保持发布包（AppImage、deb、rpm）0MB 膨胀。

## 状态与影响 (Consequences)

- 实现对主流 Linux 发行版（Ubuntu、Debian、Fedora、Arch 等）X11 与 Wayland 环境的无缝覆盖；
- 避免复杂的提权配置或外部守护进程绑定，开箱即用；
- 保持全量 82 个自动化测试持续 100% 通过。
