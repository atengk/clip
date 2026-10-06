# 领域文档规范 (Domain Docs)

工程技能在探索与分析本代码库时，如何使用与消费本仓库的领域文档。

## 探索代码库前必读

- 仓库根目录下的 **`CONTEXT.md`**，或者
- 仓库根目录下的 **`CONTEXT-MAP.md`**（若存在）—— 它指向每个上下文独立的 `CONTEXT.md`。请阅读与当前任务相关的每一个上下文文档。
- **`docs/adr/`** —— 阅读涉及你即将操作的代码区域的 ADR（架构决策记录）。在多上下文仓库中，还需检查对应 `src/<context>/docs/adr/` 中特定上下文的决策。

如果上述任何文件不存在，**请静默继续**。不要特意提示这些文件缺失，也不要提前建议创建它们。`/domain-modeling` 技能（通过 `/grill-with-docs` 和 `/improve-codebase-architecture` 触发）会在术语或架构决策真正敲定时按需延迟创建它们。

## 目录结构 (File structure)

单上下文仓库（绝大多数代码库适用）：

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-event-sourced-orders.md
│   └── 0002-postgres-for-write-model.md
└── src/
```

多上下文仓库（根目录下存在 `CONTEXT-MAP.md`）：

```
/
├── CONTEXT-MAP.md
├── docs/adr/                          ← 系统全局架构决策
└── src/
    ├── ordering/
    │   ├── CONTEXT.md
    │   └── docs/adr/                  ← 上下文专有决策
    └── billing/
        ├── CONTEXT.md
        └── docs/adr/
```

## 遵循专业术语表词汇 (Use the glossary's vocabulary)

当交付物中命名领域概念时（包括 Issue 标题、重构方案、方案假设、测试用例名称等），请严格使用 `CONTEXT.md` 中定义的术语。严禁随意替换为术语表明确避免的同义词。

如果所需的领域概念尚未收录进术语表，这是一种信号 —— 要么是你创造了项目未曾使用的生造词（请重新斟酌），要么是存在真实的领域概念缺口（请记录并在后续交由 `/domain-modeling` 完善）。

## 标明 ADR 冲突 (Flag ADR conflicts)

如果方案或修改与现有的 ADR 决策相违背，必须显式提出冲突并说明理由，严禁静默覆盖：

> _与 ADR-0007（基于事件溯源的订单系统）存在冲突 —— 但值得重新探讨，因为……_
