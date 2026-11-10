# OpenCode 时间线顺序与工具历史修复

> 历史验证记录：下述路径、命令和指标对应所注明的源码提交；当前目录与上游整合结果见 [PR 验证](opencode-upstream-pr.md)。

日期：2026-10-03。修复基线：`819a0c41c836b7745275f1bd64833343173abf4b`。
外部桌面复测确认此前三个问题已修复，本次处理新发现的两个问题。

## 问题与处理

### 流式助手消息先于用户消息

OpenCode 插件此前先通过 `Progress` 发布助手增量，到轮次完成才发布完整历史里的用户消息。
Timeline 按写入顺序分配持久游标，因此助手的第一段落在用户前面，重启不会改变已经保存的顺序。

现在，在发布每轮第一条非空助手增量前，读取该轮已经接纳的原生用户记录，
按同一个历史投影生成 `Timeline` 事件，再通过同一 FIFO 通道发送助手增量。
保留原生消息 ID、时间戳、turn ID 及客户端消息 ID；完成时同一用户记录由现有身份去重。
只解析匹配输入，其他未完成的助手/工具记录不会阻断流式输入确认。
缺失该输入时拒绝发布孤立助手内容，不重新发送模型请求。

旧记录恢复只在 OpenCode 适配层处理：Ait 的显示键从 `native:opencode:` 升级为
`native:opencode:projection-v2:`。这不是 OpenCode 的原生协议版本或消息 ID。
已有 OpenCode 会话首次核对历史时，沿用现有原子分代机制保留旧代、重建当前投影并通知客户端重置游标；
包括原本顺序正确的旧会话也会重建一次。此后的相同历史刷新保持新代与游标不变。
原生消息内容、顺序、ID、句柄和客户端消息 ID 不变，不重新请求模型，也不删除用户数据库。
Codex 实现以及所有共享 Timeline 代码均未修改，保留其原有流式和同轮追加输入语义。

### 工具历史中的无效 null

OpenCode `bash` 通常没有 `cwd`，拒绝执行时也可能没有 output。原投影将这些缺失值
写为 JSON null，而前端正式 schema 要求字符串或省略字段，导致整个历史响应校验失败。

现在 shell 仅输出有效的 `cwd`（兼容原生 `workdir`）；没有工具输出时省略可选输出。
Read 输出使用前端认可的 `content` 字段。缺失或非法的必填 command/filePath 使用
现有 unknown detail，保留原始工具输入而不制造无效的结构化字段。
协议明确允许的 null（例如完成工具的 error、unknown output）继续保留。
前端 schema 不放宽，工具审批和原生执行规则不变。

本轮没有新增领域边界或数据库 schema；显示投影版本只影响 OpenCode。1.14.46 原生 CLI 不提供实时增量的已知表现
不在这两个错误的修复范围内，不宣称本次为其新增原生流式能力。

## 回归验证

- 修复前新增的三个 OpenCode 回归失败，分别复现重启后错序、shell cwd 为 null、非法必填字段仍投影为结构化工具。
- 旧错序记录刷新回归确认，未升级显示键时现有 reconcile 会保留错误游标；升级后恢复原生顺序。
- OpenCode 多轮回归覆盖 V1/V2 离线协议夹具、原生助手尚未完成时读取已接纳用户、
  用户先于增量、客户端消息 ID、最终去重、SQLite 重开、只读历史刷新和交互恢复。
- 14 个共享工具历史样本：Rust 从真实结构的原生 JSON 投影，并逐项比对预期；
  TypeScript 使用正式 `AgentTimelineItemPayloadSchema` 校验同一批预期，覆盖成功/失败、
  缺失输出、cwd 为 null、workdir、读取结果及非法 command/filePath。
- OpenCode 专属回归验证旧错序与 null 工具投影恢复、SQLite 重开和幂等刷新。
- Codex 流式重连及同轮 steering 的真实 server 进程回归通过；
  Provider Clippy 和新增 TypeScript 的 oxlint 通过；格式与完整提交前检查结果见最终汇总。

以下命令通过 `nix develop --command` 执行，除格式工具外均使用锁定依赖：

```sh
cargo test -p server-provider local::opencode --locked --offline
cargo test -p server-provider local::opencode::projection::tests::legacy_null_tool_fields --locked --offline
npm exec --workspace=@ait/protocol -- vitest run src/messages.opencode-tool-history.test.ts --maxWorkers=1
npm run typecheck --workspace=@ait/protocol
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline -- --test-threads=1
```

最终结果：workspace Rust 测试 **1,603 passed / 0 failed / 3 ignored**；覆盖率运行同样通过 1,603 项。
其中 server 进程测试 86 项全部通过；OpenCode 定向回归共 43 项通过（首次 42 项及随后新增的 1 项旧工具恢复测试）。
前端共享契约测试 14 项通过，protocol TypeScript typecheck 通过。
`cargo fmt`、workspace Clippy（`-D warnings`）、workspace build、改动文件的 oxlint/oxfmt 与 `git diff --check` 均通过。
本轮没有重跑真实 OpenCode 发行版、在线模型或 Electron GUI。
离线协议夹具和真实 SQLite 验证不等同于完整桌面端到端验收。

## Test coverage

使用 `cargo-llvm-cov` 测量本次提交的 Rust 源码；基线为 `819a0c41c836b7745275f1bd64833343173abf4b`，
确切工作树由[覆盖率制品](opencode-history-order-and-tools-coverage.json)的 `source_sha256` 标识。
测量后只更新报告与制品。范围为完整 workspace、默认 features、macOS `aarch64-apple-darwin`、Rust 1.98.1，
采用 cargo-llvm-cov 默认文件过滤，无额外排除，不含 doctest 覆盖率。

| 范围            | 本次已覆盖 / 总行数 | 行覆盖率 |       相比基线 |
| --------------- | ------------------: | -------: | -------------: |
| workspace       |     46,562 / 50,712 |   91.82% | +0.06 个百分点 |
| server-provider |     20,922 / 22,614 |   92.52% | +0.15 个百分点 |
| OpenCode 适配层 |       2,564 / 2,963 |   86.53% | +1.25 个百分点 |

基线来自[上游合并覆盖率制品](../daemon/upstream-merge-2026-10-02-coverage.json)，是相同平台和默认 features 的历史测量，未在本轮重测。
制品同时包含逐文件统计、源码 SHA-256、命令和日志指纹，测试通过数单独记录。
本地 HTML 位于 `target/llvm-cov/html/index.html`。

```sh
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-opencode-final-coverage.json
```

三个原有 ignored 测试分别为已安装 Claude 的模型发现、Claude 在线推理、Codex 在线推理。
Windows/Linux、原生移动端、真实两版 OpenCode 与 Electron 桌面交互未在本轮验证。
OpenCode 原生进程启动/异常退出、部分 reasoning、限额及异常历史分支仍未完全覆盖；
需要测试机器人按下列步骤完成发行版和桌面验证。

## 建议测试机器人复验

在最终提交上分别使用 1.18.33 与 1.14.46：

1. 桌面创建会话并进行多轮对话；每轮用户消息应先于对应助手消息，增量和最终历史不能重复。
2. 停止并重启 Ait、切换工作区再回来，核对顺序和客户端用户气泡。
3. 允许/拒绝 shell 工具，验证结果可显示，刷新和 Retry 不再出现 schema 错误。
4. 使用修复前已保存的错序或工具历史，重启新 server 后加载/刷新历史，验证旧投影可以恢复。
5. 保持原有取消后继续输入和多次恢复回归；不要将 1.14.46 原生缺少增量单独判成本次回归。
