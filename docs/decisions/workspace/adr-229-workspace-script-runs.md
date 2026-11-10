---
id: ADR-229
status: accepted
date: 2026-11-10
paths:
  - crates/metadata/src/automation
  - crates/api/src/mcp
---

# ADR-229：Workspace 脚本运行记录、输出与参数

- 关系：扩展 [ADR-168](adr-168-scoped-setup-observation.md) 的 automation 运行时；在 [ADR-215（Bot）](../bot/adr-215-bot-permissions-and-scoped-mcp.md) 的 `observe` 组中增加两个只读工具；Guest 规则沿用 [ADR-208](../daemon/adr-208-workspace-share-guests.md)。

## 背景

本仓库用 `ait.json` 的 `verify` 脚本验证改动（见[用 Ait 开发 Ait](../../plans/ait-on-ait.md)）。
原实现把脚本的 stdout／stderr 接到空设备，`terminalId` 不对应真实终端，一次运行没有记录，也不能传参；
Agent 只能轮询 `list_workspace_scripts` 得到退出码，无法把结果作为某个提交的验证证据。

## 决策

1. **运行记录。** `workspace.script.start.request` 每次启动生成 daemon 分配的 `runId`，记录脚本名、参数、
   启动时 `HEAD` 提交与是否有未提交／未跟踪改动（`git status --porcelain`，`GIT_OPTIONAL_LOCKS=0`）、
   开始与结束时间、耗时、退出码和结果 `running|succeeded|failed|stopped`。`stopped` 只表示由 Ait
   （停止请求或 Workspace 归档）终止；外部信号终止计为 `failed`。脚本快照增加 `runId` 和 `acceptsArgs`。
2. **输出。** stdout 与 stderr 接到同一管道，由专用线程读取，按写入顺序合并。每次运行在内存中保留开头
   16 KiB 和结尾 48 KiB（测试摘要和最终错误通常在结尾），记录实际写入总字节和是否截断。
   服务可以长期运行而内存有界，因此不沿用 setup 超过 8 MiB 即终止的规则。
3. **保留范围。** 记录只在 daemon 内存中：每个脚本的当前／最近一次运行，加上每个 Workspace 最近 16 次被替换的运行。
   daemon 重启和 Workspace 归档时丢弃。与 setup 快照相同，不写入 Workspace 注册表。
4. **读取与等待。** 新增 `workspace.script.run.get.request`（按 `runId`，或按 `scriptName` 取最近一次）和
   `workspace.script.run.wait.request`（`timeoutMs` 默认 30000、最大 45000，与 `wait_for_agent` 一致）。
   等待使用 `execution_waits` 预算，在被跟踪的任务中完成，不占用短任务预算或 automation 服务锁；
   运行结束由每次运行的 `watch` 通道通知，退出检测间隔为 100 毫秒。超时返回 `timedOut: true` 并省略输出，
   避免反复等待时重复传输大段输出；不会停止运行。
5. **参数。** 只有 `ait.json` 中声明 `"acceptsArgs": true` 的脚本接受 `args`，否则拒绝，防止参数被静默忽略而产生错误证据。
   参数作为位置参数传给 `/bin/sh -lc <command> <scriptName> <args...>`：`$0` 为脚本名，命令用 `"$@"` 引用，
   参数从不拼接进命令字符串，因此不构成 shell 注入面。没有引用 `"$@"` 的命令在 `$0` 改变外行为不变。
   上限为 64 个参数、每个 4096 字节、不含 NUL。Windows 的 `cmd` 无法安全传递参数，传参时拒绝。
6. **权限。** 两个新方法对 Guest 采用 `Rule::Workspace`（与列出和启停脚本相同，只能指定共享范围内的 Workspace；
   整机共享按已注册方法放行）。MCP 新增 `get_workspace_script_run` 和 `wait_for_workspace_script`，属于 Bot 的
   `observe` 组，作用域规则与 `list_workspace_scripts` 相同：目标 Workspace 必须在授权范围内且未归档。
   启动脚本仍属于 `scripts` 组；读取他人启动的运行输出视为观察。

## 后果

- Agent 可以 `start_workspace_script(verify, args)` → `wait_for_workspace_script(runId)`，得到“哪个提交、是否有未提交改动、
  结果、耗时、输出”。`dirty: false` 且 `succeeded` 的运行才是该提交的验证证据。
- 记录不持久、不关联 PR 状态或合并门禁；这些需要另行决定存储和展示位置。
- `terminalId` 仍是逻辑标识。移动端停止脚本时调用 `killTerminal(terminalId)`，对脚本进程无效，需要客户端改用
  `workspace.script.stop.request` 或由脚本改为真实终端，另行决定。
- 每次启动额外执行两次 `git` 读取；每个运行中的脚本多一个输出读取线程和一个退出轮询线程。

## 验证

`metadata` 单元测试覆盖输出有界保留、参数边界与注入用例、运行记录生命周期、历史淘汰、`acceptsArgs` 拒绝、
环境变量和服务层的 Workspace 校验；`api` 测试覆盖 Guest 与 Bot 作用域；daemon 进程测试通过 WebSocket
完成带参数启动、等待、读取最近运行与超时参数校验。
