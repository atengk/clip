# 选用 Tauri v2 与平台抽象层 (PAL) 实现跨平台桌面架构

面对全桌面平台（Windows、macOS、Linux）的剪贴板管理诉求，我们决定采用 Tauri v2（Rust 核心系统引擎 + React / TypeScript 前端）并构建平台抽象层（Platform Abstraction Layer, PAL）。该架构通过 Rust Trait 统一抽象各操作系统的剪贴板监听、全局热键、焦点恢复与模拟粘贴，使领域状态、SQLite 持久化与 UI 层 100% 跨平台复用，同时将 7×24 小时后台静默常驻内存严格控制在 35MB 以内。

## Status

accepted

## Context

剪贴板管理器作为高频常驻系统工具，用户对内存开销、无感唤起与响应延迟极度敏感。
项目目标覆盖 Windows、macOS 与 Linux 全桌面环境，但各操作系统的底层剪贴板事件机制、模拟按键权限模型与窗口系统存在显著差异。
本地开发环境已具备完整的 Rust 1.98 与 Node.js 22 工具链，而缺少 .NET SDK。

## Considered Options

1. **Tauri v2 (Rust + Webview) + 平台抽象层 (PAL)**：常驻内存 25~35MB，启动毫秒级，无独立重型运行时依赖。通过 Rust 条件编译与 Trait 机制优雅隔离各操作系统底层 API。实施上采取 Windows 优先落地闭环、接口设计全平台对齐的敏捷策略。（采纳）
2. **Electron (Node.js + Chromium)**：全平台 Web 体验一致，但静默常驻内存高达 110~160MB，安装包超 70MB，且官方剪贴板模块缺失原生变更事件监听，违背轻量级系统工具设计准则。（放弃）
3. **.NET (WPF / WinUI 3 / MAUI)**：Windows 原生度极高，但 WPF/WinUI 3 无法跨平台至 macOS/Linux；且当前开发环境未就绪 .NET SDK。（放弃）

## Consequences

- 团队可在 React / TypeScript 舒适区快速构建交互界面，而无需承担 Electron 的巨大内存包袱。
- 首阶段依托当前开发机优先交付基于 Win32 API 的 Windows 驱动实现，后续可沿 PAL 契约无缝扩展 macOS（Cocoa/Accessibility）与 Linux（X11/Wayland）驱动，核心业务无需重构。
- 需要在 Rust 端维护各平台的底层调用安全边界与错误降级机制。
