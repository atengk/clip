# ADR 0004: Rust 单 Crate 模块化分层与无头测试接缝架构

## 状态 (Status)
已采纳 (Accepted) - 2026-10-06

## 背景 (Context)
随着剪贴板管理系统功能切片的推进，后端需承载跨平台底层 API（Win32 监听与模拟输入）、持久化存储（SQLite FTS5 与文件 Blob）、离线 OCR 识别、无痕/去重业务规则以及与前端 Webview 的 IPC 桥接。
在工程组织形式上，面临以下决策分支：
1. **多 Crate 物理拆包**（如 `clip-pal`, `clip-core`, `clip-desktop`）：各层物理隔离，但存在跨 Crate 重复配置、版本同步繁琐及 CI 编译缓存命中率下降的成本。
2. **单 Crate 扁平堆叠**：逻辑混合在 `lib.rs`，缺乏清晰边界，导致难以编写脱离桌面运行时的单元测试。
3. **单 Crate 严格模块化分层 + Trait 测试接缝**：在 `src-tauri` 保持单个统一二进制/库 Crate，通过清晰子模块划分（`pal/`、`storage/`、`engine/`、`commands/`）及 Trait 抽象解耦。

## 决策 (Decision)
我们决定采用 **单 Crate 内部严格模块化分层 + Trait 测试接缝** 架构，并在根目录由 Cargo 虚拟工作区（Virtual Workspace）统一纳管。

### 1. 模块分层与落盘目录契约
后端代码统一组织在 `src-tauri/src/`，严格遵循以下四层单向依赖架构：

```text
src-tauri/src/
├── pal/                  # 平台抽象层 (Platform Abstraction Layer)
│   ├── mod.rs            # 定义 PlatformDriver 抽象 Trait
│   ├── windows.rs        # Windows 原生驱动 (AddClipboardFormatListener, SendInput)
│   └── mock.rs           # 测试桩驱动 (MockPlatformDriver)，用于无头测试
├── storage/              # 持久化与存储层
│   ├── mod.rs            # 存储接口定义
│   ├── sqlite.rs         # SQLite + FTS5 全文检索引擎
│   └── blob.rs           # 图片与大文本文件系统 Blob Store
├── engine/               # 核心业务引擎 (无头核心，纯纯 Rust 逻辑)
│   ├── mod.rs            # ClipboardEngine 状态机
│   ├── deduplication.rs  # 去重与 Bump-to-Top 策略
│   ├── privacy.rs        # 隐私脱敏与 Clipboard Viewer Ignore 过滤
│   └── ocr.rs            # 图像文本抽取调度
├── commands/             # Tauri IPC 命令接入层
│   ├── mod.rs            # 命令分发与注册
│   └── clipboard.rs      # 前端可调用的 TauriCommand 处理函数
├── lib.rs                # 应用生命周期初始化与托盘/事件总线装配
└── main.rs               # 桌面应用启动入口
```

### 2. 依赖与可见性约束
- **单向流**：`commands -> engine -> storage / pal`。底层模块严禁反向依赖上层模块。
- **内部封装**：模块间共享结构体与方法默认使用 `pub(crate)`，避免非必要公开导出。
- **测试接缝**：核心业务引擎 `ClipboardEngine` 仅依赖 `Arc<dyn PlatformDriver>` 与存储接口，不感知任何 Tauri 桌面窗口或 Webview 运行时。

### 3. 测试策略契约
- 所有端到端业务流（如“收到系统剪贴板通知 -> 格式读取 -> 去重判断 -> 存储落盘 -> 回填通知”）必须在 `cargo test` 下通过 `MockPlatformDriver` 在毫秒级时间内确定性运行，绝不允许依赖真实 Windows GUI 桌面环境。

## 影响与效果 (Consequences)
- **正面影响**：
  - 研发效率极高，避免了多 Crate 拆包带来的 Cargo 依赖重复解析与版本对齐心智开销。
  - 保证无头核心 100% 可测，CI 跨平台静态检查（Linux/Windows）均可在数秒内完成。
  - 为工单 #2 ~ #10 的代码落盘提供了确定性的路径约定。
- **权衡与妥协**：
  - 模块边界依赖团队遵守 Rust 规范（`pub(crate)` 与接口分层），需在代码评审中关注不越级调用的原则。
