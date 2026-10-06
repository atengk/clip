# Issue 追踪器：GitHub

本仓库的 Issue 与需求规格（Specs）均以 GitHub Issues 形式进行管理。所有操作统一使用 `gh` CLI 命令行工具执行。

## 约定与规范 (Conventions)

- **创建 Issue**：`gh issue create --title "..." --body "..."`。多行正文推荐使用 Heredoc 语法。
- **查看 Issue**：`gh issue view <number> --comments`，可通过 `jq` 过滤评论并获取标签（labels）。
- **列出 Issue**：`gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'`，可搭配适当的 `--label` 与 `--state` 过滤参数。
- **发表评论**：`gh issue comment <number> --body "..."`
- **添加/移除标签**：`gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **关闭 Issue**：`gh issue close <number> --comment "..."`

从 `git remote -v` 中自动推断仓库信息 —— `gh` 在本地 Git 克隆仓库内运行时会自动完成推断。

## Pull Requests 作为分流与需求来源 (Pull requests as a triage surface)

**PRs as a request surface: no.** _（若本仓库将外部 PR 视为功能需求，请设为 `yes`；`/triage` 技能会读取此标识）_

当设置为 `yes` 时，PR 遵从与 Issue 相同的标签与状态流转，使用对应的 `gh pr` 命令：

- **查看 PR**：`gh pr view <number> --comments`，并通过 `gh pr diff <number>` 查看变更差异。
- **列出外部 PR 进行分类分流**：`gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments`，然后仅保留 `authorAssociation` 为 `CONTRIBUTOR`、`FIRST_TIME_CONTRIBUTOR` 或 `NONE` 的记录（剔除 `OWNER`/`MEMBER`/`COLLABORATOR`）。
- **评论 / 标签 / 关闭**：分别使用 `gh pr comment`、`gh pr edit --add-label`/`--remove-label`、`gh pr close`。

GitHub 在 Issue 和 PR 之间共享同一个编号空间，因此单个裸编号 `#42` 可能代表其中任何一种 —— 优先尝试 `gh pr view 42`，若失败则回退至 `gh issue view 42`。

## 当技能提及“发布到 issue tracker”时 (When a skill says "publish to the issue tracker")

创建一个 GitHub issue。

## 当技能提及“获取相关 ticket”时 (When a skill says "fetch the relevant ticket")

执行 `gh issue view <number> --comments`。

## Wayfinding 导航协同操作 (Wayfinding operations)

由 `/wayfinder` 技能使用。**导航图（Map）**是一个单独的 Issue，**子任务（Child）**作为其关联的工单（Tickets）。

- **Map（全景图）**：带有 `wayfinder:map` 标签的独立 Issue，记录 Notes / Decisions-so-far / Fog 正文。执行 `gh issue create --label wayfinder:map`。
- **Child ticket（子工单）**：作为 GitHub sub-issue 关联到 Map 的 Issue（通过 sub-issues 端点调用 `gh api`）。若未开启 sub-issues 功能，则在 Map 正文的任务清单中添加该子任务，并在子任务正文顶部注明 `Part of #<map>`。标签格式为 `wayfinder:<type>`（如 `research`/`prototype`/`grilling`/`task`）。一旦被认领，该工单将指派给执行开发者。
- **Blocking（阻塞依赖）**：GitHub 的**原生 Issue 依赖** —— 界面可见的标准依赖呈现。通过 `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>` 添加依赖边，其中 `<blocker-db-id>` 是阻塞工单的数字类型**数据库 ID**（通过 `gh api repos/<owner>/<repo>/issues/<n> --jq .id` 获取，_并非_ `#number` 或 `node_id`）。GitHub 接口返回 `issue_dependencies_summary.blocked_by`（仅包含未解决的阻塞项 —— 作为门禁）。若原生依赖不可用，可在子任务正文顶部降级使用 `Blocked by: #<n>, #<n>`。当所有阻塞项都关闭时，工单解除阻塞。
- **Frontier query（前沿查询）**：列出 Map 下所有处于 open 状态的子工单（通过 `gh issue list --state open`，限定范围为 Map 的 sub-issues / 任务列表），剔除存在未解决阻塞项（`issue_dependencies_summary.blocked_by > 0` 或 `Blocked by` 行中有未关闭 Issue）或已有指派人的工单；按 Map 中的排布顺序优先获取第一项。
- **Claim（认领）**：`gh issue edit <n> --add-assignee @me` —— 会话的首个写入操作。
- **Resolve（解决）**：执行 `gh issue comment <n> --body "<answer>"`，然后 `gh issue close <n>`，接着在 Map 的 Decisions-so-far 章节追加上下文索引（摘要 + 链接）。
