# ADR 0006: 发布资产命名规范与 Windows 安装向导品牌化体系

## 状态 (Status)
已采纳 (Accepted) - 2026-10-07

## 背景 (Context)
在 `v1.0.0` 正式版发布后，用户反馈与分发复盘暴露出以下体验与工程痛点：
1. **发布资产命名碎片化**：Tauri 原生与底层打包器输出的文件名风格不一（连字符与下划线混用，小写应用名，如 `clip-1.0.0-1.x86_64.rpm` 与 `clip_1.0.0_x64-setup.exe`），缺乏统一平台标识度；
2. **安装向导视觉复古简陋**：Windows 默认 NSIS 模板呈现 90 年代灰色向导框，缺乏品牌标识与现代化视觉层次；
3. **安装语言与权限摩擦**：缺乏地道的中英双语自适应机制；全机安装模式强制触发 UAC 提权，增加普通用户初次安装阻力。

## 决策 (Decision)

我们决定确立 **发布资产规范 (`ReleaseAssetSpec`)** 与 **安装向导品牌化套件 (`InstallerBranding`)**。

### 1. 全平台发布资产统一语义命名契约 (`ReleaseAssetSpec`)
在 GitHub Actions 发布流水线（Stage 3 聚合阶段）中，对构建产物实施规范化重命名，模板为：
`Clip-{Version}-{OS}-{Arch}[-Setup].{ext}`

全量保留 7 类跨平台核心资产：
- **Windows**:
  - `Clip-{Version}-Windows-x64-Setup.exe` (NSIS 安装程序)
  - `Clip-{Version}-Windows-x64.msi` (WiX 企业分发包)
- **macOS (Apple Silicon)**:
  - `Clip-{Version}-macOS-arm64.dmg` (DMG 拖拽安装镜像)
  - `Clip-{Version}-macOS-arm64.app.tar.gz` (便携独立应用包)
- **Linux**:
  - `Clip-{Version}-Linux-x86_64.AppImage` (跨发行版免安装)
  - `Clip-{Version}-Linux-amd64.deb` (Debian / Ubuntu 安装包)
  - `Clip-{Version}-Linux-x86_64.rpm` (Fedora / RHEL 安装包)
- **哈希清单**:
  - `checksums.txt` 基于上述标准化文件名重新计算 SHA-256 并聚合挂载。

### 2. 进程与产品标识升级
- `tauri.conf.json` 中 `productName` 规范调整为 `"Clip"`，使任务管理器进程名、桌面快捷方式与安装目录统一规范为 `Clip`。

### 3. Windows NSIS 品牌化与交互规范 (`InstallerBranding`)
- **视觉风格与配色体系 (Modern UI 2 现代浅色规范)**：
  - **安装向导主题**：采用现代纯净浅色（Light Theme），与 Windows 桌面环境与系统原生向导背景自然融合；
  - **侧边栏大图 (`sidebarImage`, 164×314 BMP)**：高明度纯净浅白/冷灰细腻渐变底色，居中镶嵌立体紫蓝品牌 Logo 与深炭灰 Typography，杜绝突兀深黑硬边；
  - **顶部导航条 (`headerImage`, 150×57 BMP)**：纯白 Banner 与微缩剪贴板图标；
  - **高清应用图标**：注入抗锯齿高清 `.ico`。
- **免提权安装与目录页规范 (`installMode: "currentUser"`)**：
  - **路径呈现**：真实展开当前用户路径（如 `C:\Users\<User>\AppData\Local\Programs\Clip`），杜绝抽象代码变量；
  - **心智提示**：配合清晰安全标识 `🛡️ 当前用户目录（免管理员提权）`，消除 UAC 弹窗阻力并保障无缝自动静默升级；
  - **容量指示**：严格对齐商业级 NSIS 规范，实时显示 `所需空间：约 45.2 MB` 与 `可用空间`。
- **中英双语自适应 (`languages: ["English", "SimpChinese"]`)**：
  - 静默自动探测当前 Windows 系统区域语言，英文环境全英文、中文环境全中文，消除中英混杂（如杜绝 `立即运行 Clip (Recommended)`，中文严谨对应为 `立即运行 Clip (推荐)`）。
- **完成页闭环与初次唤出指引 (`MUI_PAGE_FINISH`)**：
  - **默认勾选动作**：`[✓] 立即运行 Clip (推荐)`，并预置桌面快捷方式与开机自启动选项；
  - **初次唤出指引卡片**：明晰标注全局热键 `Alt + V` 与系统托盘静默守护，点击完成后主面板直接居中唤出首次亮相。

### 4. 应用程序主悬浮面板架构与视觉契约 (`AppWindowBranding`)
- **架构决策**：**完全采纳人机工效精修版变体 A (Raycast / Spotlight 流线型)**（660 × 480~520px）；
- **二层紧凑头部 (Two-Layer Compact Header)**：
  - 整合模式切换胶囊（`📋 历史` / `⚡ 短语`）与抽屉预览按钮至搜索主栏右侧，垂直高度压缩 35%（从 122px 降至 80px），首屏完整容纳 8 条记录且完全不截断；
- **全槽位严格对齐 (Strict Slot Alignment)**：
  - 强制执行统一 28×28 类型/缩略图卡片槽位（文本 `📄`、防窥 `🛡️`、代码 `</>`、图片 `🖼️`、链接 `🔗`、短语 `⚡`），彻底消除左侧视觉锯齿与扫视抖动；
- **消除技术黑话与认知去噪**：
  - 彻底剔除 `FTS5` 等底层开发名词，转换为用户友好提示 `共 8 条历史 • 按 1~8 快捷回填`；
- **键盘优先闭环 (Keyboard-First Ergonomics)**：
  - 原生支持 `↑`/`↓` 焦点漫游、`Space` 抽屉即时展开/收起、`Enter` 回填粘贴、`1~8` 数字直达粘贴、`Tab` 动作调色板、`Esc` 逐级退出。

## 影响与效果 (Consequences)
- **发布规范度提升**：对外 Release 列表整洁统一，消除用户识别与选择混淆；
- **安装转化率提升**：免提权 + 现代化视觉向导显著改善 Windows 用户第一眼印象；
- **键盘生产力闭环**：主面板兼顾高信息密度与极简视觉美感，达成零学习成本的高频流转体验。
