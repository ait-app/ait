# Oh My Pi

Oh My Pi 是独立 Provider `omp`，由独立的 Rust 适配器统一运行 `omp --mode acp --no-pty`。
它的安装、配置、权限、工具和会话均由 OMP 管理，不依赖 Ait 的 Pi 适配器。

## 安装与配置

按 [OMP 官方说明](https://github.com/can1357/oh-my-pi)安装并登录，
在运行 daemon 的主机确认 `omp --version` 可用。必须使用包含[随附 ACP 扩展](../../third-party/oh-my-pi/README.md)的 OMP 18.12.6 或兼容实现。
在 Ait 中选择 Oh My Pi；非默认路径在启动 daemon 前设置：

```sh
export AIT_SERVER_OMP_BIN=/absolute/path/to/omp
```

默认原生配置位于 `~/.omp/agent`。原生会话通过 ACP `session/resume` 恢复。
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

ACP 初始化必须协商版本 1 的原生特性与 session-settled 扩展。
逻辑输出和单个历史文件上限均为 64 MiB，模型目录最多 8192 项；超过上限明确失败。
输入不因超时或断开自动重发。prompt 成功响应和匹配 session/turn 的 `_session/settled`
共同决定整轮完成，中间 `agent_end` 不提前释放回合。
暂不提供原生会话/文件回退、单独控制子 Agent、终端组件 UI、每会话 MCP/工具策略覆盖、
宿主工具/URI callback 注册和输出 schema。OMP 自己的工具、MCP 与扩展仍由本机配置加载。
认证在原生 CLI 完成，不通过 Ait 问答收集登录秘密。

决策见 [ADR-180](../decisions/providers/adr-180-omp-native-rpc-provider.md)，
验证结果见 [报告](../reports/providers/omp-native-provider.md)。

## ACP 与旧会话迁移

ACP 是必需执行协议，没有 Protocol 控件，也不在能力不足时回退 RPC。
旧配置中的 `featureValues.transport: "rpc"` 仅用于兼容，执行仍使用 ACP。
旧 RPC 会话从原生历史恢复为 ACP，保留 session ID、模型、已有特性设置和用户消息回执。
模型/推理、四个权限档位、自动压缩/重试、fast/slow、审批、问答、steering、取消、
子 Agent、图片和 token/cost 均继续可用；当前模型不支持的档位不会展示。

正式会话由 OMP 加载本机 MCP，Ait 不接收或复制其认证凭据。
草稿发现使用 `--no-session`，不加载本机 MCP，不写入会话或修改 fast/slow 等原生设置。
修改系统提示后继续同一份原生历史。图片的原生 blob 引用按有界只读规则转换为 Ait 图片工件。

原生 ACP 补丁同时提供后台续跑与明确结算：Ait 核对本轮 prompt 响应及 settled，
再读取原生历史并发布终态。缺少必需扩展时在发送输入前拒绝，不能继续使用 RPC。
安装与验证步骤见[补丁说明](../../third-party/oh-my-pi/README.md)，迁移决策见
[ADR-190](../decisions/providers/adr-190-omp-acp-settlement.md)。
Ait 不自动替换系统 OMP 或重启运行中的 daemon。
