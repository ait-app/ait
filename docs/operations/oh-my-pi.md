# Oh My Pi

Oh My Pi 是独立 Provider `omp`。Ait 为每个 Agent 启动一个官方 `omp --mode rpc-ui` 进程，
通过 OMP 原生 JSONL RPC 通信，并把原生事件直接映射为 Ait 的会话事件。设计与 Paseo 的 OMP Provider
相同，不经过 ACP。安装、认证、模型、工具、扩展、MCP 和会话文件都由 OMP 管理，与 Ait 的 Pi 适配器无关。
本页说明配置和限制；完整行为规则见 [Oh My Pi 现行规格](../architecture/omp.md)。

## 安装与版本

按 [OMP 官方说明](https://github.com/can1357/oh-my-pi)安装并登录，在运行 daemon 的主机上确认
`omp --version` 可用。最低支持官方 `18.12.6`（RPC v2 与原生空闲判定），当前按 `18.18.8` 验证；
不需要定制构建或源码补丁。程序不在 PATH 中时，在启动 daemon 前设置：

```sh
export AIT_SERVER_OMP_BIN=/absolute/path/to/omp
```

默认原生配置位于 `~/.omp/agent`，`PI_CONFIG_DIR`、`PI_CODING_AGENT_DIR` 和 XDG 目录按 OMP 自己的规则生效。
`OMP_PROFILE`（即使为空也优先）或 `PI_PROFILE` 选择 profile；OMP 不接受的名称会使 OMP 退出，
Ait 也拒绝它。Ait 不读取、不复制 OMP 的登录凭据，也不通过 Ait 的问答收集登录信息。

Ait 不再提供 OMP 的 ACP 入口。0.2.7 及更早版本的 `daemon omp-acp`（归档中为 `aitd omp-acp`）
已删除，没有替代命令（[ADR-248](../decisions/providers/adr-248-omp-paseo-native-rpc.md)）。

## 启动与模式

| Ait 模式          | OMP `--approval-mode` | 行为                                                      |
| ----------------- | --------------------- | --------------------------------------------------------- |
| Full Access       | `yolo`                | 普通读、写和执行自动允许，原生显式 deny/prompt 规则仍有效 |
| Allow File Writes | `write`               | 读和文件写入允许，命令执行需审批                          |
| Always Ask        | `always-ask`          | 写入和执行需审批                                          |
| Ask and Remember  | `always-ask-cached`   | 未授权的文件访问及执行需审批，可显式选择原生记忆范围      |

未选择模式时按 Full Access 启动。原生审批不是操作系统沙箱；沙箱 Workspace 另由 Ait 的写入限制约束，
见下文[沙箱 Workspace](#沙箱-workspace)。

- 新会话以 `--model <provider/id>` 和 `--thinking <级别>` 启动，系统提示经 `--append-system-prompt` 传入。
- 恢复会话使用 `omp --session <会话文件>`，先沿用会话记录的模型；只有所选模型出现在 OMP 的模型列表中，
  才切换过去。会话记录的模型已无法解析时，OMP 拒绝恢复，Agent 报告启动失败；需先在 OMP 中修复该模型。
- 模型、推理级别和 Fast 在空闲时原地修改。修改模式、系统提示或 provider options 时，Ait 在回合之间
  以同一会话文件重启 OMP，并保留当前模型、推理级别和 Fast。
- 进程意外退出时当前回合失败，并附带退出原因；下一次输入时 Ait 从会话文件重新启动 OMP。

## 特性与斜杠命令

唯一的特性开关是 Fast。只有 OMP 的状态报告了 Fast 时才显示；Fast 已开启但当前模型不适用时，开关会附带说明。
已接通：动态模型与推理选择、系统提示、流式文本/推理/工具、token/cost、图片与文本附件、取消和追加输入。
图片总是交给 OMP。当前模型不接受图片时，OMP 自己保存图片，并以一条隐藏消息告诉模型没有配置视觉模型；
Ait 不显示这条消息，用户消息照常显示图片。需要模型分析图片时，在 OMP 中配置 `modelRoles.vision`。

命令列表先列出 Ait 内置的命令，再列出 OMP 自己的命令和技能。内置命令在输入队列之外执行，
长时间压缩不会阻塞其他请求：

| 命令                             | 作用                                                       |
| -------------------------------- | ---------------------------------------------------------- |
| `/compact [说明]`                | 手动压缩上下文，显示自己的开始与完成行；同一时间只允许一个 |
| `/autocompact [on\|off\|toggle]` | 设置自动压缩；不带参数时切换当前设置                       |
| `/handoff [说明]`                | 交接上下文                                                 |
| `/steer <消息>`                  | 空闲时开始新回合；回合进行中时插入当前运行                 |
| `/follow-up <消息>`              | 空闲时开始新回合；回合进行中时排在当前运行之后             |

内置命令失败时显示 `[Error] …`，且不接受图片或附件。其他斜杠文本（包括 `/skill:<名称>`）作为普通输入发送。

回合进行中的追加输入以 OMP `steer` 发送；以 `/` 开头的文本不作为追加输入，而由 Ait 作为下一条输入投递。
OMP 在写入后拒绝追加输入时，显示警告通知，当前回合继续，该输入不会重发。

停止使用 OMP 的 `abort`，Ait 不等待原生清理：最多等待五秒确认，超时后报告失败并关闭进程。
OMP 会保留已排队的追加输入并立即继续处理它们，此时 Ait 结束被停止的回合，把后续运行显示为新的回合，
追加输入保留原来的消息身份。被停止或失败的回合会把运行中的子 Agent 标为已取消；任一回合结束时仍未收到
结果的工具都会结束，OMP 已结束的保留其结果，其余标为已取消。

## 审批与问答

- 工具审批：OMP 标题为 `Allow tool: bash|edit|write` 的选择框显示为带命令、文件或写入内容的工具审批。
  OMP 提供的会话、目录和文件授权范围作为额外按钮显示；只有用户明确选择这些按钮，OMP 才会保存授权。
  其他选择框显示为普通问答。审批卡片上的命令和写入内容最多显示 16 KiB。
- `ask` 工具：每个选择字段显示为一张问题卡，支持单选、多选和 “Other” 自由文本；提交后，Ait 自动回答
  OMP 的后续逐项提问。
- 插件对话：`select` 为单选，`confirm` 为 Yes/No 问题，`input` 为文本（placeholder 提到 optional 或
  skip 时可以跳过），`editor` 为多行文本并保留换行。原生标题、选项说明和输入提示都会保留。
- `notify` 显示为对应级别的通知，`open_url` 显示为带链接的消息。运行中的元数据读取和追加输入不等待
  原生确认，所以 OMP 等待回答时不会卡住审批或问答界面。

## 历史与会话目录

会话目录取 `PI_CODING_AGENT_SESSION_DIR`；未设置时为 OMP 数据根目录下的 `sessions`，数据根目录为已存在的
XDG `omp[/profiles/<名称>]` 或 agent 目录。与 OMP 18.18.8 一致，不读取 `settings.json` 的 `sessionDir`。
图片 blob 始终在数据根目录的 `blobs` 下。

会话列表、导入、已有会话恢复和当前分支历史均可用。列表中的标题依次取 OMP 标题、可读的文件名、首条输入。
历史读取会跳过损坏或不完整的行，并按 OMP 的 `activeLeafId` 选择当前分支。尚未写入原生文件的空 Agent
可以重新打开；已提交过输入但会话文件丢失时，恢复报错，不会新建对话，也不会重发输入。
Ait 不改写原生会话文件。

## 子 Agent

原生子 Agent 的状态和消息进入 Ait 的子 Agent 界面和各自的时间线。标题为 “agent · 模型 (provider)”，
描述取自任务说明，ID 为 `<父会话 ID>/<任务名>`；task 卡片以同一 ID 指向子 Agent。
OMP 把子 Agent 记录写在 `<父会话文件去掉扩展名>/<任务名>.jsonl`，Ait 通过父会话的 task 结果关联它们，
因此也能在历史中发现和查看。子 Agent 不能单独控制。

## 沙箱 Workspace

沙箱 Workspace 中的 OMP 进程可以写配置根目录（`~/.omp` 或 `$HOME/$PI_CONFIG_DIR`）、`PI_CODING_AGENT_DIR`
和 `$XDG_DATA_HOME/omp`，因此会话文件、`blobs`、`agent.db`（含登录刷新）、日志、缓存和运行状态照常写入。
以下条目对受限进程只读，因为之后不受限的 OMP 进程（包括 Ait 的模型发现与命令探测）会遵从或执行它们：

- agent 目录中的 `config.yml`、`config.yaml`、`settings.json`、`models.yml`、`models.yaml`、`models.json`、
  `mcp.json`、`.mcp.json`、`.env`，以及 `extensions`、`hooks`、`tools`、`plugins` 目录；
- 配置根目录的 `.env`、`plugins`、`natives`、`python-env`，以及 XDG data 根目录的 `natives`、`plugins`、
  `python-env`；
- 默认 profile 的会话还不能写 `profiles`。

受限会话中 OMP 保存设置会失败，OMP 只记录警告。只有旧版 `settings.json` 而没有 `config.yml` 时，
OMP 的迁移同样被拒绝并记录警告，会话照常运行，下一次不受限启动时完成迁移。
安装或升级 OMP 后，原生 addon 需要先由不受限进程解出（运行一次 `omp`，或让 Ait 刷新模型列表），
否则受限 Agent 启动时报告 `Failed to load pi_natives native addon`。

Linux 首次受限启动会为缺失的受保护条目建立占位：目录为空目录，设置与模型文件写入 `{}`。占位的
`config.yml`、`models.yml` 会遮住只写在 `config.yaml`、`models.yaml` 或未迁移的 `models.json` 中的配置，
使用这些文件名的用户应先改为 `.yml`。

## 用量

回合进行中每 3 秒读取一次 OMP 的 `get_session_stats`；回合结束时（先于结果发布）以及压缩、交接和模型变化后
再读取一次；只在数值变化时发布。OMP 的统计只覆盖保留下来的消息，所以压缩或交接后总数会下降。
不支持该命令的 OMP 改用 `get_state.contextUsage` 加消息累计的用量。

## 诊断

诊断只运行 `omp --version`，不启动会话进程，并报告：程序路径、版本、是否达到 18.12.6、当前 profile、
配置根目录、agent 目录、agent 数据库是否存在，以及 XDG data/state/cache 根目录。报告不包含 stderr、
环境变量值或凭据。找不到可执行文件时只提示检查 PATH 或 `AIT_SERVER_OMP_BIN`。

## Provider options

| 选项                                  | 作用                                                |
| ------------------------------------- | --------------------------------------------------- |
| `smolModel`、`slowModel`、`planModel` | 以 `--smol`、`--slow`、`--plan` 传给 OMP 的角色模型 |
| `rpcTimeoutMs`                        | 等待 `ready` 和每个请求的超时，1 至 2147483647 毫秒 |

模型值不能为空，不超过 1024 字节，且不含控制字符。其他键都会被拒绝，包括 Paseo 的 `sessionDir`。
修改 provider options 会在回合之间重启 OMP。

## 限制

- 默认等待 `ready` 20 秒、每个请求 60 秒；读取模型目录不设超时，但恢复会话时等待模型列表不超过请求
  超时，超时则保留会话原有的模型。
- 单条原生 stdout 行和 v2 重组帧上限 64 MiB；无效帧被丢弃，不会中断会话。
- 单个会话文件上限 64 MiB、131072 个条目；会话列表每个文件只读开头 64 KiB 与末尾 256 KiB，全部列表共用
  256 MiB 读取预算。模型目录上限 8192 项。
- 同时打开的对话最多 32 个，每个回合的追加输入最多 128 个，系统提示最多 64 KiB。超过 64 KiB 的
  问答（工具审批除外）被取消并显示警告，回合继续。
- 每个会话实时跟踪 256 个子 Agent，超出时先替换最早结束的子 Agent。持久句柄只保留最新 4096 条
  用户消息的客户端身份。
- 后台请求写入失败或超时后，该 OMP 连接作废，当前回合失败；下一次输入由运行时重新启动 OMP。
- 输入不因超时或断开自动重发。不支持输出 schema、每会话 MCP server 或工具策略覆盖；OMP 自己的工具、
  MCP 和扩展仍按本机配置加载。

## 已知缺口

- 尚未提供：宿主工具与 MCP 桥接、会话回退（`branch`）、迟到拒绝的追加输入重新排队、按会话实时更新的
  命令列表缓存、终端组件 UI。OMP 请求宿主工具或 URI 处理器时，当前回合失败。
- 同一回合中先 `/follow-up` 后 `/steer` 时，两条消息的客户端 ID 可能对调（OMP 先投递 steer）。
- HTML `ask` 中预先勾选的复选项无法回放。
- 沙箱中仍可写的提示类内容（`AGENTS.md`、`SYSTEM.md`、skills、commands、rules、prompts、agents、memories）
  会影响之后的会话。命名 profile 的会话无法保护 `profiles/<名称>` 下自己的配置。macOS 上，把受保护条目的
  可写祖先目录改名、修改后再改回，可以绕过 Seatbelt 的按路径保护（其他 Provider 相同）。

决策见 [ADR-248](../decisions/providers/adr-248-omp-paseo-native-rpc.md)
（修订 [ADR-180](../decisions/providers/adr-180-omp-native-rpc-provider.md)），
早期验证见[历史报告](../archive/reports/providers/omp-native-provider.md)。
