---
status: accepted
date: 2026-10-08
---

# Windows 资源管理器重命名免失焦守护与常规聚焦双模架构决策 (ADR-0011)

## 背景与问题 (Context)

在经历 ADR-0009 统一为单一 660×520 主面板并尝试“失焦后回填时发送 F2 补偿”之后，实际使用中暴露了致命缺陷：
- Windows 资源管理器（Explorer）或桌面的内联重命名控件属于原生 Win32 `Edit` 控件；
- 当第三方窗口（Clip 主面板）抢占系统前台焦点（`SetForegroundWindow` / `set_focus`）时，Explorer 处理 `WM_KILLFOCUS` 消息的原生默认行为是**立即销毁该内联 `Edit` 控件并退出重命名编辑态**；
- 所谓“回填时发送 F2 补偿”，在用户按 `Alt+V` 唤出的**一瞬间**，重命名输入框就已经销毁关闭了，用户肉眼直观感受到的是“按了 Alt+V 就立刻失焦退出了重命名”，体验极差；
- 坚决遵守产品架构约束：**全局保持唯一的 660×520 主面板，严禁再次引入第二种迷你紧凑卡片**。

必须从**唤出的一瞬间**就阻断焦点转移，实现真正的“零失焦常驻”。

## 方案选型与决策 (Decision)

我们决定实施“**重命名态免失焦守护 + 常规态正常聚焦**”的智能感知双模架构：

### 1. 呼出前态精准嗅探 (In-place Renaming Probing)
在快捷键呼出窗口前，通过 Win32 API 采集当前活动前台窗口：
- 检查顶层窗口类名是否为资源管理器或桌面（`CabinetWClass` / `ExploreWClass` / `Progman` / `WorkerW`）；
- 获取当前活动 GUI 线程信息（`GetGUIThreadInfo`），检测当前持有键盘焦点的子控件类名是否为 `Edit`；
- 若匹配，精确标记当前处于就地重命名态（`IS_EXPLORER_RENAMING = true`）。

### 2. 重命名态：免激活展现与光标闪烁保持 (No-Activate Display)
当处于重命名态时：
- 主面板窗口动态赋予扩展样式 `WS_EX_NOACTIVATE`（`0x08000000`）；
- 使用 `ShowWindow(hwnd, SW_SHOWNOACTIVATE)` 配合 `SetWindowPos(..., SWP_NOACTIVATE | SWP_SHOWWINDOW)` 原子呈现；
- **绝不调用** `window.set_focus()` 和 `SetForegroundWindow`；
- 前端通过 `panel-shown` 接收到 `mode: "renaming"`，搜索框设置为 `readOnly` 且不调用 DOM `focus()`；
- 资源管理器的文件名编辑框全程未收到 `WM_KILLFOCUS`，原光标持续在原地闪烁。

### 3. 独立线程底层输入拦截守护 (Low-Level Hook Guardian)
免激活态下，由于 Clip 窗口无系统焦点，用户的击键默认会落入资源管理器编辑框：
- 启动独立 Win32 消息循环守护线程，按需挂载 `WH_KEYBOARD_LL` 与 `WH_MOUSE_LL`；
- 拦截方向键 `↑`/`↓`：广播 `global-key-nav` 通知前端移动列表高亮项，阻止光标在原编辑框内乱移；
- 拦截数字键 `1`~`9`：广播对应数字动作，前端直接就地直贴对应序号历史条目；
- 拦截 `Enter`：广播确认动作，就地粘贴当前高亮条目；
- 拦截 `Esc`：安全收起面板并卸载钩子，阻断 Esc 传递给 Explorer，保护重命名不被意外取消；
- 拦截外部鼠标点击：鼠标点击在 Clip 外部时自动隐藏面板并卸载钩子；
- 严格防御系统热键：检测到 `Alt` 或 `Ctrl` 处于按下态时（如 `Alt+Tab`、`Alt+V`、`Ctrl+C`），一律不予拦截放行。

### 4. 常规态：全功能即打即搜与自动聚焦
当在常规应用（代码编辑器、浏览器、记事本等）下唤出时：
- 窗口确保移除 `WS_EX_NOACTIVATE` 样式并卸载钩子；
- 正常调用 `window.show()` 与 `window.set_focus()`；
- 搜索框自动聚焦，支持拼音检索、高级快捷操作与全量功能。

### 5. 精准就地回填 (Precise In-Place Pasting)
回填时隐藏面板并安全卸载钩子。由于重命名控件全程未曾失焦，无需多余的 `F2` 重激活操作，直接模拟发送 `Ctrl+V`，剪贴内容精准就地注入当前编辑框中。

## 架构影响 (Consequences)

- **优势**：
  - 彻底解决了 Windows 文件重命名时按 `Alt+V` 瞬间失焦关闭的顽疾，重命名光标全程不灭；
  - 严格保持全局唯一的 660×520 主面板，无碎片化多形态维护负担；
  - 独立 Hook 线程与消息循环隔离，按需安装/卸载，不影响主应用响应；
  - 72 项单测与全量 E2E 毫秒级回归全绿。
- **权衡**：
  - 重命名态下 Clip 处于免激活模式，不能在面板内打字搜索，但支持极为高效的 1~9 直贴、方向键漫游、Enter 确认与鼠标点选。
