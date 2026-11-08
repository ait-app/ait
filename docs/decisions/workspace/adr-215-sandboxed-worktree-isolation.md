# ADR-215：沙箱化 Worktree 隔离级别

- 状态：已接受
- 日期：2026-11-06
- Linux 扩展：[ADR-223](adr-223-linux-worktree-sandbox.md) 自 2026-11-08 起补充 Linux 后端；Linux 差异与部署要求以该 ADR 为准。
- 关系：在 [ADR-060](adr-060-workspace-create-worktree.md) 的 Paseo 自有 worktree 之上增加隔离级别；
  Provider 测试归属沿用 [ADR-179](../providers/adr-179-provider-owned-tests.md)。

## 背景

`isolation: "worktree"` 只给 Agent 一个独立 checkout，不限制 Agent 进程写入什么位置。Codex 的
`workspace-write` 和 Claude Code 的 `sandbox.*` 只约束各自的 shell 工具，原生文件编辑工具仍由权限
提示控制，不受内核限制。需要一个在操作系统层面包住整个 Provider 进程树的隔离级别：读取不受限，
写入只能落在 worktree 及其运行所必需的位置。网络不在本决策范围内。

## 决策

### 建模与持久化

1. `domain::workspace::sandbox::WorkspaceSandbox` 只有一个取值 `worktree_writes`。它作为可选字段
   `sandbox` 存在于 `PersistedWorkspaceRecord`、`WorkspaceDescriptorPayload` 和
   `WorkspaceWorktreeSource` 上，缺省时不序列化。
2. 选择可选字段，而不是新增 `PersistedWorkspaceKind` 变体：旧记录无需迁移即可读取；旧 daemon 读到
   新记录时忽略未知字段，不会因为无法识别 kind 而拒绝加载整个 Registry；`kind` 仍准确描述底层
   放置方式（linked worktree），现有对 kind 的穷举匹配和客户端 `workspaceKind` 枚举不受影响。
   代价是降级到旧 daemon 后，该 Workspace 的 Agent 会在不受限的情况下运行，并且旧 daemon 重写记录时会
   丢掉该字段。
3. MCP `create_workspace` 接受 `isolation: "sandboxed-worktree"`，复用全部 worktree 参数（`mode`、
   `worktreeSlug`、`branchName`、`baseBranch`、`branch`、`prNumber`、`forge`），并在 worktree source
   上写入 `sandbox: "worktree_writes"`。isolation 只有一个取值，“沙箱 + local”的组合无法表达；
   `local` 搭配任何 worktree 参数、以及未知的 isolation 值，都返回 `InvalidMessage`。此前未知值会被
   当作 worktree 处理。
4. 客户端通过 `workspace.create.request` 的 worktree source 传 `sandbox: "worktree_writes"`，与 MCP
   走同一条路径。旧 daemon 会忽略未知字段并创建不受限的 worktree，所以客户端只在 server info
   声明 `workspace-sandbox-v1` 时提供“沙箱化 Worktree”选项；该 feature 只在装有 metadata 的
   macOS/Linux daemon 上声明。记住的选项在不支持的主机上降级为普通 worktree，触发按钮显示实际级别；
   Schedule 尚不能请求沙箱，同样以普通 worktree 作为初始值。

### 分层

- **domain**：`WorkspaceSandbox` 表示隔离级别；`WriteConfinement::for_workspace` 从 Workspace 记录
  推导出 confinement（优先使用记录的 `worktreeRoot`）；`WritePolicy` 是纯值，包含可写目录树、
  可写路径前缀和受保护路径，`WritePolicy::worktree` 固定了 Git 相关规则。
- **应用层**（`provider::service::agent_manager::confinement`）：创建、恢复、还原以及 rewind
  原生进程之前，用 Workspace Registry 查出 confinement 并写入 `AgentSessionSpec::confinement`。该字段
  不随 Agent 持久化，每次启动都按 Workspace 记录重新计算。适配器未声明
  `AgentClient::supports_write_confinement` 时返回 `ConfinementUnsupported`，失败关闭；Registry
  读取失败同样拒绝启动。
- **适配器**（`provider::local::sandbox`）：解析真实路径和 Git 目录，生成平台 profile 并包装命令。
  `sandbox::command` 是唯一的平台接缝。macOS 后端是 `sandbox::seatbelt`，Linux 后端是 `sandbox::linux`（bubblewrap）；其他平台拒绝启动。

### macOS 机制

在 Provider 命令前加上 `/usr/bin/sandbox-exec -p <profile>`。profile 以内联字符串传入而不是 `-f`
文件，这样受限 Agent 无法改写之后启动所加载的 profile，也省去临时文件的生命周期管理。profile 结构为
`(version 1) (allow default) (deny file-write*) (allow file-write* …) (deny file-write* <受保护路径>)`。
Seatbelt 按最后匹配的规则生效。不采用全部默认拒绝，因为那会破坏 dyld、Mach 服务和 TTY。路径中的
`"` 与 `\` 会被转义；含控制字符或非 UTF-8 的路径拒绝渲染。

写入允许清单以 `@anthropic-ai/sandbox-runtime` 的 macOS profile 为参照：

| 类别         | 路径                                                                                                                                                                                                              |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Worktree     | 记录的 worktree 根目录（realpath）                                                                                                                                                                                |
| Git          | `.git` 文件指向的私有 gitdir 和 `commondir` 解析出的公共 Git 目录（realpath），保证 `git commit` 可用                                                                                                             |
| 临时与设备   | `$TMPDIR`（realpath）、`/private/tmp`、`/private/var/folders`、`/dev/null`、`/dev/zero`、`/dev/{u,}random`、`/dev/dtracehelper`、`/dev/ptmx`、`/dev/stdout`、`/dev/stderr`、`^/dev/tty`、`^/dev/pty`、`^/dev/fd/` |
| Claude Code  | `CLAUDE_CONFIG_DIR` 或 `~/.claude`；`~/.claude.json*` 前缀（含原子写入产生的 `.tmp.*` 兄弟文件）；`~/.cache/claude`、`~/Library/Caches/claude-cli-nodejs`                                                         |
| Codex        | `CODEX_HOME` 或 `~/.codex`                                                                                                                                                                                        |
| Pi           | `PI_CODING_AGENT_DIR` 或 `~/.pi/agent`；`PI_CODING_AGENT_SESSION_DIR`                                                                                                                                             |
| Oh My Pi     | `PI_CONFIG_DIR` 或 `~/.omp`；`$XDG_DATA_HOME/omp`                                                                                                                                                                 |
| OpenCode     | XDG data、state、cache、config 下的 `opencode` 目录                                                                                                                                                               |
| MiniMax Code | `~/.minimax`                                                                                                                                                                                                      |

环境变量先读 Agent 的覆盖值，再读 daemon 环境。尚不存在的状态目录按最近的已存在祖先解析 realpath；
含 `..` 的状态路径直接拒绝。

以下路径即使位于允许范围内也保持只读，因为之后不受限的进程会执行或遵从其中内容：worktree 的
`.git` 指针文件、公共 Git 目录的 `config` 和 `hooks`、私有 gitdir 的 `config.worktree`、Claude 的
`settings.json` 与 `plugins`、Codex 的 `config.toml`、Pi 的 `settings.json`、OpenCode 的
`opencode.json`／`opencode.jsonc`／`config.json`、MiniMax 的 `config.yaml`、`bin`、`plugins`、`shims`。
因此 `git config`、`git push -u`、`git branch --set-upstream-to` 这类写配置的命令会失败。

Provider 状态目录没有改到每个 Workspace 独立的位置：认证信息就存放在这些目录里（Claude 在 macOS
上的钥匙串条目还和配置目录绑定），恢复会话和读取原生历史也依赖它们，改到别处会让沙箱 Workspace
里的 Agent 掉登录、丢失历史。因此只能允许写入真实的状态目录，并用上述受保护路径收窄范围。

Unix socket：`allow default` 下 connect 不受影响；在允许清单以外的位置 `bind` 新 socket 会被拒绝，
允许的临时目录中可以。

### 嵌套沙箱

已经处于 Seatbelt 下的进程再次调用 `sandbox_apply` 时，内核返回 `Operation not permitted`，嵌套
沙箱并不会按交集组合。实测 `codex exec --sandbox workspace-write` 在外层 profile 下每条工具命令都
以该错误失败；Claude Code 的 `sandbox.enabled` 同样以退出码 71 失败。因此在受限的 Workspace 中：

- Codex 的 `thread/start`、`thread/resume`、`thread/fork` 以及每个 turn 的 sandbox policy 固定为
  `danger-full-access`，审批策略保持不变。写入边界由外层 profile 提供；Codex 自带沙箱的网络限制和
  只读模式在这里不再生效。
- Claude Code 通过 `--settings` 合并 `sandbox.enabled: false`，其他 sandbox 设置保留。

### 不支持的平台

macOS/Linux 以外的平台，带 sandbox 的 worktree 创建请求会在任何 Git 操作之前被 `metadata` 拒绝，返回
“Sandboxed worktree isolation requires macOS or Linux”。如果 macOS/Linux 以外的平台上已有沙箱记录，启动 Agent 时适配器
返回 `Unavailable` 并记录错误日志，不会退化为不受限运行。

### Provider 覆盖范围

Claude Code、Codex、Pi、Oh My Pi、OpenCode、MiniMax Code 的交互会话进程都会被包装，包括会话内的
重启以及 Claude／Codex 的 rewind。Antigravity 和 DeepSeek Harness 未声明支持，在沙箱 Workspace 中
创建或恢复时返回 `ConfinementUnsupported`。

## 已知缺口

- `open`、`osascript`、经 Mach lookup 访问的 launchd 服务，以及其他代表调用方执行操作的系统服务，
  都可以在 profile 之外完成写入。
- 状态目录中受保护文件以外的可执行内容依然可写，例如 Claude 的 hooks 目录、Codex 的 skills。
  不受限的会话之后可能执行这些内容。
- 公共 Git 目录整体可写，包括其他 worktree 的 metadata、所有 refs 和对象。
- `/private/tmp`、`/private/var/folders` 由用户的所有进程共享。
- 工具缓存位于 home 目录下（例如 `~/.cargo`、`~/.npm`），需要写入它们的命令会失败，目前没有额外
  可写路径的配置项。
- 不改变运行时进程的辅助进程不受限制：模型与能力发现、命令列表、用量查询、只读历史回放和摘要生成。
- 以 Directory source 打开同一个 worktree 目录会得到一个普通 Workspace，不继承沙箱。

## 后果与验证

沙箱级别随 Workspace 记录持久化，并在描述符上可见；所有受支持 Provider 的交互进程都在同一个写入
边界内运行，不支持的 Provider 失败关闭。

验证覆盖：domain 中的记录兼容性、描述符投影与 policy 规则；profile 渲染，包括转义、realpath 和
公共 Git 目录；MCP 参数解析；Codex／Claude 的嵌套沙箱处理；应用层 confinement 解析；以及一个
macOS 集成测试，在生成的 profile 下运行 `/bin/sh -c`，断言 worktree 内的写入成功，同级目录、`$HOME`
和受保护的 `.git/config` 写入失败。

另外在外层 profile 下用真实的 Codex 0.166.0 手动验证：`danger-full-access` 时 worktree 写入和
linked worktree 中的 `git commit` 成功，同级目录、`$HOME`、公共 `.git/config` 和 `.git` 指针写入
被拒绝。
