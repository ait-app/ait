# Oh My Pi

Oh My Pi 是独立 Provider `omp`，固定通过 Ait 的 Rust ACP 桥接层运行官方 `omp --mode rpc-ui`。
它的安装、配置、权限、工具和会话均由 OMP 管理，不依赖 Ait 的 Pi 适配器。

## 安装与配置

按 [OMP 官方说明](https://github.com/can1357/oh-my-pi)安装并登录，
在运行 daemon 的主机确认 `omp --version` 可用。兼容基线为官方 `18.12.6` RPC v2，无需定制构建或源码补丁。
在 Ait 中选择 Oh My Pi；非默认路径在启动 daemon 前设置：

```sh
export AIT_SERVER_OMP_BIN=/absolute/path/to/omp
```

默认原生配置位于 `~/.omp/agent`。原生会话恢复使用 `omp --resume`。
模型、token、MCP、工具及扩展配置由 OMP 维护。Ait 的凭据不进入原生权限配置。

## 权限与特性

| Ait 模式          | OMP 原生模式        | 行为                                                      |
| ----------------- | ------------------- | --------------------------------------------------------- |
| Full Access       | `yolo`              | 普通读、写和执行自动允许，原生显式 deny/prompt 规则仍有效 |
| Allow File Writes | `write`             | 读和文件写入允许，命令执行需审批                          |
| Always Ask        | `always-ask`        | 写入和执行需审批                                          |
| Ask and Remember  | `always-ask-cached` | 未授权的文件访问及执行需审批，可显式选择原生记忆范围      |

没有显式模式覆盖时保留 OMP 本机配置。审批展示原生选项和范围，
用户必须明确选择原生长期授权选项才会保存授权。
原生审批不是操作系统沙箱。

已接通动态模型/推理选择、系统提示、自动压缩/重试、fast/slow 服务档位、
流式文本/推理/工具、token/cost、图片与文本附件、取消和追加输入。
服务档位和推理选项仍由当前原生模型验证。

OMP `ask` 使用结构化问答，支持单选、多选、含逗号的选项与自由文本。
原生子 Agent 的状态与消息进入 Ait 子 Agent 界面和各自时间线。
技能/命令发现、会话列表、导入、已有会话恢复和当前分支历史同样可用。

## 输出分类

Provider adapter 根据原生角色和事件类型生成共享时间线，客户端不匹配英文正文或插件名称。

| 原生输出                         | 共享展示                    | 规则                                                                                                                          |
| -------------------------------- | --------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Assistant text / thinking        | 回复 / Thinking             | 保留原生消息与轮次身份。                                                                                                      |
| Tool result                      | 工具结果                    | 沿用工具调用身份，输出在工具详情中展开。                                                                                      |
| OMP `async-result`               | Background job 工具结果     | 单任务使用原生 jobId；批次与缺少 job 元数据的结果保留一个独立结果卡。内部继续执行指令与长日志仅保留在详情中，不充当助手回复。 |
| Extension `notify`               | info / warning / error 通知 | 保留原生通知级别，不充当助手正文。                                                                                            |
| 显式可见的其他插件消息、命令输出 | 原生插件展示内容            | 继续尊重原生 display 标记；不将未知插件内容按正文猜测成后台任务。                                                             |

OMP 的实时输出与历史恢复使用相同投影；后台任务失败只影响该工具结果，
不把整轮模型执行判为失败。模型上下文和 OMP 原生会话文件保持原样。
已有正文形式的后台结果在原生历史重新校验时替换为工具结果；客户端仍沿用
Timeline generation 替换协议。标准 ACP 没有独立通知类型，通知通过独立的
Provider notification 工具条目传递，保留级别和内容，不伪装成 agent_message_chunk。
这与 Codex 命令输出、Claude tool result、OpenCode ACP tool update 和 Pi bashExecution
经 adapter 映射为工具结果的边界一致。Pi 的可见自定义插件内容仍遵循其原生展示标记。

## 传输与边界

ready 后协商 v2，校验并重组 stdout 分块，支持大于 1 MiB 的原生输出。
输入单帧限 1 MiB，逻辑输出和单个历史文件限 64 MiB；超过上限明确失败。
输入不因超时或断开自动重发。`prompt_result` 与 `session_settled` 控制整轮状态，
中间 `agent_end` 不提前释放当前回合。
遗漏结算通知时，通过同一原生进程的明确 `get_state.isSettled` 核对收尾；
状态未知或仍有后台工作时继续等待。停止请求写入后即返回，原生取消确认等待最多五秒，
超过后报告失败并关闭受管进程；进程清理还需要短暂等待。

暂不提供原生会话/文件回退、单独控制子 Agent、终端组件 UI、每会话 MCP/工具策略覆盖、
宿主工具/URI callback 注册和输出 schema。OMP 自己的工具、MCP 与扩展仍由本机配置加载。
认证在原生 CLI 完成，不通过 Ait 问答收集登录秘密。

决策见 [ADR-180](../decisions/providers/adr-180-omp-native-rpc-provider.md)，
验证结果见 [报告](../reports/providers/omp-native-provider.md)。

## ACP 桥接与插件

ACP 是固定入口，没有 Protocol 控件；恢复同样经过 Ait 的桥接层。
旧 `transport: rpc/acp` 选择仅做兼容，实际句柄标明 `transport: acp`、`nativeTransport: rpc`。
桥接层负责 ACP→官方 RPC 转换，对话、控制、子 Agent 和后台任务由同一个原生实例持有。
无需定制 OMP、源码补丁或插件配合；这与直连 OMP 自带 ACP 不同。

OMP 中已安装的插件继续按官方规则加载。命令、工具、问答、自定义消息与原生额外帧
采用通用接口处理，不按插件名称、路径或命令写分支。具体能力和 TUI 限制见
[架构与兼容表](../architecture/omp-acp-bridge.md)。

外部 ACP 客户端可使用同一桥接引擎：

```sh
daemon omp-acp --omp-bin /absolute/path/to/omp
```

不传 `--omp-bin` 时使用 `AIT_SERVER_OMP_BIN` 或 PATH 中的 `omp`。
该子命令仅服务 stdin/stdout JSONL，不启动 HTTP daemon、不要求 Host 登录或创建 API token。
Ait 内部已使用同一引擎，不需要另外启动这个进程。

决策见 [ADR-190](../decisions/providers/adr-190-omp-acp-settlement.md)，
当前官方 CLI 与插件验证见[报告](../reports/providers/omp-acp-settlement-2026-10-25.md)。
