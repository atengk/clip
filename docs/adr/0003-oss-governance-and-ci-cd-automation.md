# 采用 atengk/oss-template 标准化开源工程底座与自动化 CI/CD 发版体系

为了确保项目从 Day 1 起就遵循工业级开源软件标准，我们决定深度集成 `atengk/oss-template` 开源模版工程底座。通过标准化引入 GitHub Actions CI 流水线、`git-cliff` 自动化发版、结构化 Issue/PR 模版、统一代码规范（`.editorconfig` / `.gitattributes`）以及 Apache-2.0 许可证，为 `clip` 剪贴板管理器构建高质量、无人值守的自动化交付体系。

## Status

accepted

## Context

`clip` 作为面向全桌面平台（Windows、macOS、Linux）的开源系统工具，需要长期的社区协作与高频发版。
若缺乏统一的工程化底座，将面临 PR 规范失控、换行符跨平台错乱（CRLF/LF 污染）、版本发版更新日志手工维护繁琐以及安装包打包人工操作易错等工程质量风险。
`atengk/oss-template` 提供了成熟的开源规范套件与 GitHub Actions 最佳实践。

## Considered Options

1. **深度集成 atengk/oss-template（采纳）**：
   - 继承其基于 Conventional Commits 的语义化 PR 校验；
   - 针对 Tauri v2 双核技术栈（Node/pnpm + Rust/Cargo）定制分层 CI 矩阵（Ubuntu 快速无头校验 + Windows 原生编译守卫）；
   - 融合 `git-cliff` 与 `@tauri-apps/tauri-action`，打 Tag 自动生成结构化更新日志并自动编译上传 `.msi` / `.exe` 安装包及 SHA-256 校验清单；
   - 统一全套工程治理规范（`.editorconfig`、`.gitattributes`、Issue/PR 模版、`CONTRIBUTING.md`、Apache-2.0）。
2. **纯手工维护与零外部规范（放弃）**：零模版约束，导致每次发版需要手动编写 GitHub Release、手动编译上传安装包，协作成本与出错率极高。

## Consequences

- 团队与外部贡献者遵循统一的 Conventional Commits 提交规范与分支管理模型。
- 发布新版本只需本地推送语义化标签（如 `git tag v0.1.0 && git push origin v0.1.0`），GitHub Actions 自动完成编译、打包、校验和计算与 Release 挂载，彻底释放维护精力。
