# Workspace 自动化：`ait.json`

`ait.json` 声明 Worktree 的 setup／teardown 命令和可按名称启停的脚本。daemon 在每次操作时重新读取，
不缓存；修改后下一次列出或启动脚本即生效。实现位于 `crates/metadata/src/automation/`，
决策见 [ADR-229](../decisions/workspace/adr-229-workspace-script-runs.md) 与
[ADR-230](../decisions/workspace/adr-230-worktree-teardown.md)。

## 文件位置与读取规则

- 读取 Workspace 目录（`cwd`）下的 `ait.json`。从仓库子目录创建的 Worktree，`cwd` 是 Worktree 中对应的子目录。
- `ait.json` 不存在时才读取旧文件名 `paseo.json`。`ait.json` 存在但无效时报错，不回退。
- 必须是普通文件，不能是符号链接；不超过 1 MiB；顶层必须是 JSON 对象。文件不存在等同于空配置。
- 通过项目设置保存时，`worktree.setup`／`teardown` 必须是字符串或字符串数组，`scripts` 的每项必须是对象，
  `worktree.servicePorts` 只接受 `range`（如 `"3000-3999"`）和 `portScript`。daemon 运行时不使用 `servicePorts`；
  无效脚本条目在运行时被忽略，不使整个文件失效。

### 新 Worktree 中的配置来源

创建 Worktree 时，如果新检出中没有 `ait.json`（也没有被检出的 `paseo.json`），daemon 把来源检出中的
`ait.json`（或旧文件名）复制进新 Worktree，作为未跟踪文件；分支自带的 `ait.json` 不会被覆盖。
因此分支没有该文件时，使用的是来源检出当时的配置，它引用的脚本文件在该分支上可能不存在，
此时命令按普通 shell 失败处理。需要兼容旧分支的命令应先判断文件，例如
`test ! -f scripts/x.mjs || node scripts/x.mjs`。

## 字段

```json
{
  "worktree": {
    "setup": ["npm ci", "node scripts/seed-cache.mjs"],
    "teardown": "docker compose down"
  },
  "scripts": {
    "verify": { "command": "node scripts/verify.mjs \"$@\"", "acceptsArgs": true },
    "web": { "type": "service", "command": "npm run dev -- --port $AIT_WORKTREE_PORT" },
    "api": { "type": "service", "command": "cargo run", "port": 8080 }
  }
}
```

| 字段                         | 含义                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------------------------------------------- |
| `worktree.setup`             | 字符串或字符串数组。新建 Worktree 后按顺序执行；空白项跳过                                            |
| `worktree.teardown`          | 同上。归档并删除 Ait 拥有的 Worktree 之前执行                                                         |
| `scripts.<name>.command`     | 必填，非空 shell 命令。`<name>` 区分大小写，是启停和读取运行记录的键                                  |
| `scripts.<name>.type`        | `"service"` 表示长期运行的服务；其他值或缺省都是一次性脚本                                            |
| `scripts.<name>.port`        | 仅服务有效，1–65535，作为列表中显示的端口；缺省时显示 Workspace 端口。daemon 不探测端口、不做反向代理 |
| `scripts.<name>.acceptsArgs` | 只有 `true` 才允许调用者传参，见下文“参数”                                                            |
| `metadataGeneration.*`       | `title`、`branchName`、`commitMessage`、`pullRequest` 的 `instructions`，供生成标题、分支名等使用     |

## 执行环境

所有命令（setup、teardown、脚本）都以 `/bin/sh -lc <command>` 执行（Windows 为 `cmd /D /S /C`），
工作目录是 Workspace 的 `cwd`，标准输入为空设备。Unix 上命令在独立进程组中运行，停止时整组终止。
注意 `-l` 会读取登录 shell 配置。

| 变量                                                       | 值                                                |
| ---------------------------------------------------------- | ------------------------------------------------- |
| `AIT_SOURCE_CHECKOUT_PATH`（`PASEO_SOURCE_CHECKOUT_PATH`） | 主检出根目录；非 Worktree Workspace 为其 `cwd`    |
| `AIT_ROOT_PATH`（`PASEO_ROOT_PATH`）                       | 同上                                              |
| `AIT_WORKTREE_PATH`（`PASEO_WORKTREE_PATH`）               | Worktree 根目录；非 Worktree Workspace 为其 `cwd` |
| `AIT_BRANCH_NAME`（`PASEO_BRANCH_NAME`）                   | 当前分支；未知时为空字符串                        |
| `AIT_WORKTREE_PORT`（`PASEO_WORKTREE_PORT`）               | daemon 为该 Workspace 分配的本机空闲端口          |

两组名称的值相同；`PASEO_*` 为兼容旧配置保留。Workspace 端口在 daemon 进程内首次需要时分配，
setup、脚本和 teardown 共用，归档后仍保留到 daemon 退出；daemon 重启后会重新分配。

## 生命周期

### Setup

- 仅在新建 Worktree Workspace 后启动；普通目录 Workspace 不运行 setup。
- 来自其他仓库的变更请求（fork PR）检出被标记为不受信：setup、脚本启动和 teardown 都被阻止，
  直到用户通过 `workspace.setup.run.request` 批准，批准后立即运行 setup。
- 命令依次执行，遇到第一个失败即停止。每条命令最长 30 分钟，输出超过 8 MiB 或超时会终止其进程组。
  每条命令的转录保留开头和结尾共 64 KiB。
- 状态与转录只保存在 daemon 内存中，可通过 `workspace.setup.status.request` 读取或订阅，daemon 重启后丢失。
- 归档 Workspace 会取消正在运行的 setup。

### 脚本

- `workspace.script.list.request` 列出配置中的脚本以及配置已删除但仍在运行的进程，包含
  `lifecycle`、`exitCode`、`runId` 和 `acceptsArgs`。
- 同一 Workspace 的同一脚本同时只能运行一个实例，重复启动返回错误；不同 Workspace 互不影响。
- `workspace.script.stop.request` 终止进程组。归档 Workspace 和 daemon 退出时也会停止其全部脚本。
- `terminalId` 只是进程的逻辑标识，不对应可附着的终端；输出通过下文的运行记录读取。

### 运行记录

每次启动都记录一次运行，`workspace.script.start.request` 的结果中 `script.runId` 是其标识。

| 字段                       | 含义                                                                     |
| -------------------------- | ------------------------------------------------------------------------ |
| `commit`、`dirty`          | 启动时 `HEAD` 提交，以及是否有未提交或未跟踪的改动；非 Git 目录为 `null` |
| `args`                     | 调用者传入的参数                                                         |
| `startedAt`、`finishedAt`  | RFC 3339 时间                                                            |
| `durationMs`               | 结束后的耗时                                                             |
| `outcome`                  | `running`、`succeeded`（退出码 0）、`failed`、`stopped`（由 Ait 停止）   |
| `exitCode`                 | 退出码；被信号终止时为 `null`                                            |
| `output`                   | stdout 与 stderr 按写入顺序合并，保留开头 16 KiB 和结尾 48 KiB           |
| `outputBytes`、`truncated` | 实际写入总字节数，以及 `output` 是否省略了中间部分                       |

- `workspace.script.run.get.request` 按 `runId` 读取，或只给 `scriptName` 读取该脚本最近一次运行；运行中也可读取当前输出。
- `workspace.script.run.wait.request` 等待指定运行结束，`timeoutMs` 默认 30000、最大 45000。
  结束时返回完整记录与输出；超时返回 `timedOut: true` 且省略 `output`，运行继续。
- 记录只保存在 daemon 内存中：每个脚本的最近一次运行，加上每个 Workspace 最近 16 次被替换的运行。
  daemon 重启或归档 Workspace 后记录丢失。

MCP 工具 `start_workspace_script`、`get_workspace_script_run`、`wait_for_workspace_script` 提供相同能力，
见 [MCP 工具](mcp.md)。

### 参数

只有声明 `"acceptsArgs": true` 的脚本接受 `args`，否则启动被拒绝。参数作为 shell 的位置参数传入：
`$0` 是脚本名，`$1`… 依次是各参数，命令需要显式引用 `"$@"`，例如
`"command": "node scripts/verify.mjs \"$@\""`。参数不会被拼进命令字符串，其中的 `;`、`$(...)`、
空格等都按字面传给程序。最多 64 个参数，每个不超过 4096 字节，不能含 NUL。Windows 上不支持传参。

### Teardown

- 归档会删除 Ait 拥有的 Worktree 时运行：先关闭其中的 Agent、终端和脚本，再运行 teardown，最后删除检出。
  仍有其他未归档 Workspace 引用该 Worktree 时不删除，也不运行 teardown。
- 使用创建该 Worktree 的 Workspace 的 `cwd` 和变量；与 setup 相同的 30 分钟／8 MiB 限制；遇到第一个失败即停止。
- 不受信且未批准的 Workspace 不运行 teardown。
- teardown 失败（包括配置无效）只记录到 daemon 日志，归档照常完成并删除检出。输出不保存。
- 归档因其他清理失败而重试时，teardown 可能再次运行，命令应当可以重复执行。
