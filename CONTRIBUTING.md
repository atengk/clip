# 贡献指南 (Contributing Guide)

感谢你关注并愿意为 `clip`（极轻量桌面剪贴板管理器）贡献力量！为了保持高效协作与高质量的代码维护，请在提交代码前仔细阅读以下规范。

---

## 1. 协作与分支模型

本项目遵循标准的 **GitHub Flow** 工作流：

1. **Fork 本仓库** 到你个人的 GitHub 账号；
2. **基于 `main` 分支拉取新的特性分支**：
   ```bash
   git checkout -b feat/your-feature-name
   # 或者缺陷修复分支
   git checkout -b fix/issue-description
   ```
3. 在本地完成修改，确保自测通过并补充相应测试用例；
4. 提交更改并推送到你的远程分支：
   ```bash
   git push origin feat/your-feature-name
   ```
5. 在 GitHub 上向本仓库的 `main` 分支发起 **Pull Request**。

---

## 2. Commit 提交信息规范

本项目遵循 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/) 规范，统一采用以下格式：

```text
<type>(<scope>): <subject>
```

### 常用类型说明

| 类型 | 说明 | 示例 |
| :--- | :--- | :--- |
| `feat` | 新增功能或特性 | `feat(core): 支持按顺序出队的队列连贴模式` |
| `fix` | 缺陷与 Bug 修复 | `fix(win32): 修复极速回填前台焦点未释放的偶发竞态` |
| `docs` | 仅文档更新或修改 | `docs(adr): 补充 ADR-0003 开源工程治理记录` |
| `style` | 代码格式调整（空格、分号等，不影响逻辑） | `style: 规范 Tailwind CSS 布局类名` |
| `refactor` | 代码重构（既非新增特性也非修复缺陷） | `refactor(pal): 优化 ClipboardWatcher 平台适配契约` |
| `perf` | 性能优化 | `perf(search): 优化 SQLite FTS5 拼音分词检索响应耗时` |
| `test` | 增加或重构单元测试与集成测试 | `test(engine): 增加 PayloadGuard 大文本截断边界单测` |
| `build` | 构建系统、外部依赖或脚手架调整 | `build: 升级 Tauri v2 核心依赖版本` |
| `ci` | CI/CD 流水线与 GitHub Actions 脚本修改 | `ci: 增加 Windows 原生构建矩阵检查` |
| `chore` | 其他琐碎杂项（不改动源码与测试） | `chore: 更新 .gitignore 忽略规则` |
| `revert` | 恢复或回滚此前的某次历史提交 | `revert: feat(ocr): 回退特定模型提取变动` |

---

## 3. Pull Request 流程

- **PR 标题规范**：PR 标题必须同样遵循 [Conventional Commits](#2-commit-提交信息规范) 格式（如 `feat: 新增能力` 或 `fix: 修复缺陷`），CI 会对其进行自动化合规校验；
- **模版填写**：发起 PR 时，请按模版完整填写变更背景、解决的问题以及关联的 Issue（如 `close #2`）；
- **CI 绿灯**：确保 CI 流水线测试全部处于通过状态；
- **审查与合并**：代码审查（Code Review）提出修改意见后，在原分支继续提交即可自动同步至 PR；合并后特性分支将被删除。

---

## 4. 本地开发与测试规范

1. **环境依赖**：
   - Node.js `22+` / pnpm `11+`
   - Rust `1.90+` / Cargo
2. **代码检查与测试运行**：
   - 前端代码检查：`pnpm lint`
   - Rust 工作区语法与质量检查：`cargo check && cargo clippy`
   - 运行无头核心单元测试（毫秒级验证）：`cargo test --lib`
3. **领域契约与架构决策**：
   - 编码与命名请务必遵循项目 [CONTEXT.md](./CONTEXT.md) 中定义的 22 项统一领域术语；
   - 后端模块划分与测试接缝遵循 [ADR-0004](./docs/adr/0004-internal-modular-architecture-and-test-seams.md)；
   - 架构重大变更请前置审阅与更新 [docs/adr/](./docs/adr/)。
