---
status: accepted
date: 2026-10-08
---

# 单实例互斥守护、静默无黑框运行与网络韧性更新架构决策

## 背景与问题 (Context)

在 Windows 平台日常运行与更新流程中，用户反馈了以下三项影响原生体验的顽固问题：

1. **启动闪烁控制台黑框 (Console Flashing)**：
   - 每次启动应用或托盘菜单初始化查询自启状态时，屏幕均闪现黑色控制台窗口；
   - 根因在于原逻辑调用了 `std::process::Command::new("reg")`，Rust 标准库在 Windows 下派生子进程未赋予 `CREATE_NO_WINDOW` 标志，导致系统瞬时创建并销毁 `conhost.exe` 窗口。
2. **重复多开进程堆积 (Multiple Instances Proliferation)**：
   - 缺少单实例互斥机制，用户多次点击桌面图标会创建多个独立 `Clip.exe` 进程；
   - 多个进程同时争夺系统热键 `Alt+V`、重复托盘图标并引发 SQLite 锁竞争。
3. **更新下载失败与变更日志排版粗糙 (Update Fragility & Raw Markdown)**：
   - 前端通过 Webview `fetch` 直连 GitHub Releases 资产，受国内网络连通性与 CORS/重定向限制，极易出现 `Failed to fetch` 抛错；
   - 变更日志直接输出原始 Markdown 语法标记（裸露 `###`、`**`），可读性较差。

## 方案选型与决策 (Decision)

经架构推演（`/grill-with-docs`），我们决定实施如下三位一体的加固方案：

### 1. 彻底消除启动黑框 (Silent Subsystem)
- **Win32 原生注册表 API**：在 `src-tauri/src/engine/autostart.rs` 中，用 Windows 原生 API（`RegOpenKeyExW` / `RegQueryValueExW` / `RegSetValueExW` / `RegDeleteValueW`）完全取代 `reg.exe` 子进程，0 开销微秒级内存直存，从物理上杜绝控制台黑框生成；
- **子进程防漏网配置**：针对所有必须保留的子进程调用（如 PowerShell 对话框脚本、安装器进程），统一配置 Windows 平台专属标志 `.creation_flags(0x08000000)`（`CREATE_NO_WINDOW`）。

### 2. 单实例互斥锁与二次启动前台唤醒 (Single Instance Guard)
- **命名互斥体锁 (Named Mutex)**：应用主入口启动时尝试创建系统级互斥体 `Local\Clip_App_Single_Instance_Mutex`；
- **二次点击前台唤起**：若 `GetLastError() == ERROR_ALREADY_EXISTS`，说明已有 Clip 实例在后台常驻运行；新进程通过 Win32 API 查找既有窗口并投递展示消息（或恢复主面板置顶），随后当前新进程立即优雅静默退出，全系统永远保持单实例常驻。

### 3. 应用内更新网络韧性强化与富文本变更日志
- **Rust 后端原生流式下载**：新增后端 IPC 下载接口，直接使用底层原生 HTTP 客户端，绕过 Webview 的 CORS 限制与大文件 JSON IPC 序列化性能瓶颈；
- **国内加速镜像降级重试**：提供备用加速下载通道，若直连超时或受阻自动切换备用镜像，遭遇阻断时友好提示跳转系统默认浏览器；
- **富文本更新日志渲染**：将 raw markdown 转译为结构化分类卡片（新特性、代码重构、缺陷修复），去除生硬的标记符号。

## 架构影响 (Consequences)

- **正面收益**：
  - 彻底终结启动与查询自启动时的黑框闪烁，启动体验丝滑纯净；
  - 彻底杜绝多进程并发冲突与托盘堆积，重复点击行为符合直觉；
  - 更新下载成功率大幅提升，更新日志排版美观专业。
- **负面成本与注意事项**：
  - Win32 API 需做跨平台 `#[cfg(windows)]` 隔离以保障 macOS/Linux 测试桩编译通过。
