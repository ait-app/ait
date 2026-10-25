# Oh My Pi

Oh My Pi 是独立 Provider `omp`，由独立的 Rust 适配器运行默认 `omp --mode rpc-ui`，也可为新会话选择 `omp --mode acp`。
它的安装、配置、权限、工具和会话均由 OMP 管理，不依赖 Ait 的 Pi 适配器。

## 安装与配置

按 [OMP 官方说明](https://github.com/can1357/oh-my-pi)安装并登录，
在运行 daemon 的主机确认 `omp --version` 可用。当前适配对照 `18.12.5` RPC v2。
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

## 传输与边界

ready 后协商 v2，校验并重组 stdout 分块，支持大于 1 MiB 的原生输出。
输入单帧限 1 MiB，逻辑输出和单个历史文件限 64 MiB；超过上限明确失败。
输入不因超时或断开自动重发。`prompt_result` 与 `session_settled` 控制整轮状态，
中间 `agent_end` 不提前释放当前回合。

暂不提供原生会话/文件回退、单独控制子 Agent、终端组件 UI、每会话 MCP/工具策略覆盖、
宿主工具/URI callback 注册和输出 schema。OMP 自己的工具、MCP 与扩展仍由本机配置加载。
认证在原生 CLI 完成，不通过 Ait 问答收集登录秘密。

决策见 [ADR-180](../decisions/providers/adr-180-omp-native-rpc-provider.md)，
验证结果见 [报告](../reports/providers/omp-native-provider.md)。

## 选择 ACP

新建 Agent 的 Protocol 控件可选择 Native RPC 或 ACP；已有会话沿用创建时的协议。
API 配置为 `featureValues: {"transport": "acp"}`，恢复 metadata 同时保存 `transport`。
ACP 使用原生 default / plan 模式、模型/推理配置、审批、form 问答、steering 与取消。
审批底层策略沿用 OMP 本机配置；创建时也可用原有权限 ID 覆盖启动参数。
ACP 不展示 RPC 专用的自动压缩、重试和服务档位开关，相关配置仍由 OMP 本机维护。
OMP 原生 ACP 要求客户端提供 MCP servers；当前 ACP 适配发送空列表，不加载本机 MCP 配置。

ACP 整轮完成需要原生 `_session/settled` 扩展版本 1。补丁、协商字段、安装与验证步骤见
[OMP 原生补丁说明](../../third-party/oh-my-pi/README.md)。
它会等待能唤醒 agent loop 的后台工作，并在同一轮里继续处理结果；Ait 同时核对
prompt 响应和当前 turn 的 settled，再读取原生历史并结束运行状态。
本机旧版 18.12.6 可继续使用默认 RPC；启用 ACP 前需使用包含该补丁的原生 OMP。
Ait 不以延时模拟信号，也不自动替换系统安装。详见
[ADR-190](../decisions/providers/adr-190-omp-acp-settlement.md)。
