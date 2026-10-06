# 仓库准则与 Agent 规范 (Repository Guidelines)

## 核心工程准则 (Core Guidelines)

1. **语言规范**：代码注释、技术文档、Git 提交与对话统一使用清晰地道中文；原生代码标识符、系统命令保留英文。
2. **测试接缝**：优先在无头核心引擎与 IPC 服务层（通过 `MockPlatformDriver` 驱动）验证端到端行为，保障毫秒级确定性测试回归。
3. **安全红线**：严禁硬编码敏感凭据；代码中必须严格遵守 `Clipboard Viewer Ignore` 隐私隔离，防止机密凭据被捕获落盘。
4. **架构决策**：所有技术选型与核心设计决策严格维护在 `docs/adr/`，统一领域术语维护在 `CONTEXT.md`。

## Agent skills

### Issue tracker

GitHub Issues via `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical 5-role triage vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout (`CONTEXT.md` + `docs/adr/`). See `docs/agents/domain.md`.
