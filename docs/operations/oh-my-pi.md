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

模型设置中还可分别选择 default、smol、slow、vision、plan、commit、tiny、memory、task、
advisor、widget、image、web、speech、dictation、judge，以及 OMP 已配置的自定义角色。
候选来自原生目录，包含各模型支持的推理档位，支持图片的聊天模型会标注 images。
每个角色可恢复为 `Use OMP setting`，保留原生配置及其推理后缀。
这些选择保存到 Ait 会话配置，从下一轮生效，重启后恢复；不修改用户全局或项目配置。
当前聊天模型仍由独立模型选择器控制。角色配置使用官方 `--config`，无需 OMP 补丁。
见 [ADR-217](../decisions/providers/adr-217-omp-model-roles.md)。

OMP 成功模型目录按工作目录与运行环境隔离缓存，有界淘汰，不按五分钟过期或定时发现。
前端进入模型相关页面，打开或选择聊天模型、角色模型与思考强度，以及查看/运行模型诊断时，
使用现有目录立即展示，同时触发后台刷新。同一 Host 与工作目录的并发访问共用一次刷新；
冷启动发现尚未完成时加入现有任务，不额外启动原生探测。隐藏、禁用或断连面板不发起刷新。
刷新期间和刷新失败后都保留最后成功的目录与时间戳，下次访问或手动刷新可再次读取原生值；
角色校验、新会话创建复用目录，不因缓存年龄启动 OMP。未完成任务仍短暂轮询以收敛状态。

OMP `ask` 使用结构化问答，支持单选、多选、含逗号的选项与自由文本。
文件/命令审批和插件的 `select/confirm/input/editor/ask` 均经共享交互事件展示，
保留原生标题、选项说明和输入提示。运行中的元数据读取和追加输入不等待原生命令确认，
避免原生命令等待用户回答时阻塞审批/问答界面；追加输入的迟到拒绝仍会报告执行失败。
原生 `editor` 使用多行输入，答案保留换行。
原生子 Agent 的状态与消息进入 Ait 子 Agent 界面和各自时间线。
技能/命令发现、会话列表、导入、已有会话恢复和当前分支历史同样可用。

输入 `/` 可查找原生命令与别名；命令后输入空格可继续补全原生目录声明的子命令，
例如 `/advisor o` 的 `on`、`off` 及说明。方向键选择，Tab/Enter 接受候选；参数完成后
再按 Enter 走原生提交。Escape 关闭 OMP 补全并保留草稿。命令内继续编辑时保留后续文本。
模型、权限和会话身份冲突的 builtin 与未导出的 TUI-only 命令不会透传，也不会变成模型提示词；
例如 clear/new/exit/quit/q 会显示独立错误卡，客户端不会替换成归档或新草稿操作。
运行中的插件目录更新来自同一实例，重新打开菜单即可刷新；真实第三方同名命令仍由其 owner 执行。

原生文本对话可提交空字符串，取消保持取消语义；editor 显示原生预填内容，提交保留
缩进和首尾换行，secret 输入使用密码控件。任意插件补全函数和 TUI 组件的限制见
[命令兼容报告](../reports/providers/omp-commands-and-completion-2026-10-30.md)。

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

编辑器旁的 Native operations 提供官方 RPC 的语义操作，包括会话树、分支、fork、
导航、handoff、新会话、目标、标签、命名、统计、HTML 导出、模型预设保存/删除、账号和自有子 Agent 控制，
以及投递模式、旁路问答、shell 和 Host 语音/听写。各项独立保存输入、结果和等待状态；
失败后可修正重试，后台操作不锁住草稿。下载使用认证文件通道，桌面原生写盘完成后才显示成功。

官方扩展的状态、文本 widget、标题、编辑器和登录链接进入独立临时界面，不写成助手正文。
认证链接由用户点击，沿用官方 OAuth/原生交互；不自动打开链接或收集额外秘密。
Host 语音与听写使用 daemon 所在主机的设备，和客户端录音入口分别拥有。

暂不提供原生文件回退、任意终端组件工厂、主题及 header/footer、每会话 MCP/工具策略覆盖、
宿主工具/URI callback 注册和输出 schema。OMP 自己的工具、MCP 与扩展仍由本机配置加载。
操作、异步等待、模型缓存和恢复边界见
[ADR-218](../decisions/providers/adr-218-native-ui-and-actions.md)，
实测范围见[最终交付报告](../reports/providers/native-delivery-and-omp-roles-2026-11-02.md)。

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

## 原生 model preset

OMP 模型控件提供原生预设目录，读取全局与项目定义；同名项目预设整条优先。
打开、查看或选择模型相关控件时，目录在后台刷新，保留现有缓存与输入能力。
切换预设会一起更新当前聊天模型、各用途角色及思考强度，使用 Ait 会话的私有快照；
随后单独改模型或角色时显示为自定义配置，预设删除后已应用的会话仍保留快照。
这与 Ait Agent Profile 的保存机制相互独立，Profile 可以保存已展开的预设选择。

“原生操作”中的保存和删除通过官方 `/modelpreset save/delete` 管理指定的全局条目；
保存使用 Ait 最新的下一轮选择。项目中仍定义同名预设时会报告保留，请在项目配置中删除。
这些操作不把项目预设复制进全局，也不改写全局或项目角色设置。
原始 `/modelpreset` 命令不透传；请使用预设选择器和这两个显式保存/删除操作，避免原生写入规则与
Ait 的私有配置范围混接。

实现与约束见 [ADR-220](../decisions/providers/adr-220-omp-model-presets.md)。

## 查询命令与侧问答

`/usage`、`/context`、`/session info`、`/advisor status` 等已适配的只读形式在主任务旁执行，
结果使用独立卡片，不中断或替换主回答。裸 `/advisor` 是原生开关而非只读查询，沿用普通调度。
模型、权限、原生工作区或会话控制使用 Ait 的对应界面；`/session delete/pin` 也不透传。
TUI-only 入口不会发送给模型；例如 `/status` 是 OMP 扩展控制台的别名，`/stats` 和 `/trace`
会启动外部统计服务，都不作为可用的文本查询公开。真实第三方同名命令保持原生所有权。

`/btw 问题` 发起原生侧问答；`/btw` 查看历史。侧问答面板支持流式回答、取消、按话题追问和重连
后的原生历史恢复。主输入框可继续编辑；侧问答错误不终止主任务。预览有大小上限，完整历史仍
保存在 OMP 的 sidecar 中。官方 RPC 的 BTW 请求没有图片参数，不将带附件的输入悄悄降为纯文本。

命令投影、并发与归属规则见 [ADR-221](../decisions/providers/adr-221-omp-headless-commands-and-side-questions.md)。
