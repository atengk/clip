---
status: accepted
date: 2026-10-09
---

# Rust 原生下载引擎下沉、显式拖拽把手与纯覆盖安装升级决策 (Native Download Engine, Drag Handle & Pure Overwrite)

## 背景与问题 (Context)

在桌面端实际使用与版本分发中，发现了以下 3 项核心阻断体验痛点：
1. **应用内更新跨域 (CORS) 拦截与代理脱节**：
   - 更新安装包的下载原先通过前端 WebView2 浏览器沙箱中的 `fetch()` 发起；
   - GitHub Releases 资产直链 302 重定向到 `release-assets.githubusercontent.com`，由于未携带 `Access-Control-Allow-Origin: *` 响应头，即便系统开启了代理网络已连通，浏览器出于同源安全策略仍强行掐断连接并报错；国内镜像亦受此限制；
2. **窗口拖拽位移不明确与边缘挤压**：
   - 悬浮主面板顶部已被搜索框、模式胶囊与操作按钮高度填满，用户难以寻获空白区域按住整体拖动窗口，容易误触发边缘缩放（Resize）或误触按钮；
3. **覆盖升级触发卸载流程 (Uninstall-First)**：
   - NSIS 默认模板在检测到已安装注册表时，通常在新版本安装前调用旧版 `Uninstall.exe`，造成“先卸载再安装”的感知与潜在设置重置隐患。

## 方案选型与决策 (Decision)

经 `/grill-me-matt` 推演，采纳以下架构方案：

### 1. 窗口顶部增设精致拖拽抓手条 (Drag Handle Pill)
- 在悬浮面板 `<header>` 居中位置增设一条精致极简的抓手胶囊（`━`，尺寸 36×4px，Hover 动态延展至 48×5px）；
- 鼠标悬浮呈现 `cursor: move`，按住即可调用 Tauri 原生底层 `startDragging()`，给予用户清晰强烈的抓取位移预期，彻底杜绝拉伸误操作。

### 2. 下载引擎全面下沉至 Rust 后端 (Native Download Engine)
- 在 Rust 后端采用 `reqwest` 流式异步客户端：
  - **彻底解除 CORS 限制**：系统级进程直接发起 HTTP 请求，无任何浏览器沙箱跨域阻断；
  - **100% 自动继承系统代理**：用户开启代理时直接全速直连 GitHub 官方 Releases；
  - **自动镜像降级**：直连受阻时毫秒级自动降级至国内高可用镜像（`ghproxy.cn`, `mirror.ghproxy.com`, `ghproxy.net`）；
  - **流式进度事件桥接**：流式写入系统临时目录，并通过 Tauri Emitter 发送 `update-download-progress` 事件实时通知前端；
  - **内置 Checksum Gate 强校验**：下载完成自动做 SHA-256 哈希比对，校验通过后平滑调起静默覆盖安装。

### 3. 纯覆盖就地升级模式 (Pure In-Place Overwrite)
- 在 `src-tauri/windows/hooks.nsh` 的 `NSIS_HOOK_PREINSTALL` 宏中，清空旧版 `UninstallString` 注册表键值；
- 彻底斩断安装器对旧版卸载向导的链式调用，仅安全终止 `Clip.exe` 进程并直接覆盖写入新文件，实现 0 卸载弹窗、极速就地二进制覆盖，数据目录 100% 安全保留。

## 状态与影响 (Consequences)

- 应用内一键更新在国内外网络、有代理/无代理环境下均能秒级稳定完成；
- 窗口拖拽体验直观平滑；
- 覆盖安装不再经历卸载过程。
