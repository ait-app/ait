# ADR-223：外部 OMP 能力桥接与运行时边界

状态：Accepted（修订，撤销私有运行时例外）。日期：2026-11-09。

## 背景

原方案为接通 Vibe 安装、补丁并交付固定 OMP SDK 与 Bun，且替换所有 RPC 会话的
启动程序。该行为越过 Provider 适配器边界，也与 ADR-218 冲突。本修订撤销私有运行时，
改为 Ait 在 Rust 适配器中实现 director/worker 编排，通过公开接口连接用户安装的 OMP。
旧私有宿主的截图与报告仅代表历史实现，不能作为本实现的验收证据。

## 决策

### 外部运行时与公开桥接

- Ait 不下载、安装、复制、修改或打包 OMP SDK、Bun 或私有 OMP 运行时。
  桌面资源、独立 daemon 发布包、后台安装器和 manifest 优先替换均移除。
  仅启动用户安装的 `omp`，或用户明确配置的 `AIT_SERVER_OMP_BIN`。
  遗留私有运行时目录和环境变量不参与发现或启动；不删除用户历史数据。
- 延续 ADR-218 对未公开 TUI 工厂的限制。Ait 的 Vibe 是应用层编排，不调用或仿造
  OMP TUI 的 `sessionModes`，不声称启用了上游 TUI 模式，不依赖私有 SDK 类或补丁。
- 小型、无运行时 import 的 Ait 扩展由外部 OMP 的公开 `--extension` 加载。
  Vibe 接线使用 `registerCommand`、`getActiveTools`/`setActiveTools`、`ctx.models.resolve`、
  `before_agent_start` 和 `sendMessage`：切换 director 工具集、补充编排指令、报告模型路由、
  交付 worker 结果。扩展临时文件随连接保留并清理，不修改用户扩展目录。
  工具实现、会话归属、进程生命周期及有界状态均在 Rust provider adapter 内。

### 能力与交互

- 原生命令目录必须确认版本化的私有桥接命令 `ait-vibe-mode-v1` 已成功加载。
  已有同名外部 `vibe` 命令不被 Ait 覆盖。内部桥接命令不显示在用户命令目录中。
- 进入模式前，公开 `set_host_tools` 必须确认五个工具注册成功；扩展随后确认工具集
  切换并返回 `AIT_VIBE_V1` 的 fast/good 模型路由。缺失或无效确认拒绝进入，回滚注册，
  回滚失败则关闭连接；不把未处理的 `/vibe` 送给模型。按公开接口和版本化确认判断兼容性，
  不按 OMP 版本号猜测能力。目前实际验证的外部版本是官方 18.17.0。
- 裸 `/vibe` 切换模式，不调用模型。`/vibe <指令>` 先进入模式，再把指令作为真实
  root turn 提交；后续普通输入继续使用 director 配置。已有工具和模式在退出时恢复。
- Director 的活动工具限定为已有 `read`/`todo` 与 `vibe_spawn/send/wait/kill/list`。
  `fast` 使用原生 `@smol` 模型路由，`good` 使用 `@task`；无法解析时使用当前模型。
  Worker 继承父会话的工作目录、认证环境、审批、系统提示和沙箱约束；不继承 director 工具集。
  Thinking 使用 worker 角色的显式后缀（含角色别名）或模型的原生默认，不带入 director 的档位。
  构造 worker 时还须清除父命名预设的 `feature_values.modelPresetThinking`，避免角色未指定
  后缀时仍把父预设的值写成 worker 的原生 `defaultThinkingLevel`。
- 每个 worker 是用户安装的 OMP 启动的独立 RPC 进程，执行真实模型调用和原生工具。
  Rust actor 是其唯一写入者。`send` 在空闲时继续同一会话，运行时发送 steering；
  `wait` 异步等待状态，不阻塞父会话审批。完成结果通过公开扩展消息回到 director，
  原生 `agent_start`/settlement 映射为可观察的主回合。审批仍必须走原生策略，Ait 不自动批准。
- 协商原始 host-tool 通道的外部 ACP 客户端拥有自己的 host tools，因此该连接不启用
  Ait 的 Vibe 编排，避免 `set_host_tools` 覆盖调用方工具。
  兼容旧外部桥接的 hello+builtin 通道仍按原协议门控，不由 Ait 安装任何实现。

### 会话、资源与取消

- 新 worker 经公开 `new_session(parentSession)` 建立原生父子关系。在提交 worker prompt
  和登记只读子节点前，读取原生 session header 核对父日志；恢复也先核对 cwd、原生 ID、
  provider 和父日志。不得按任意日志路径恢复或接受外来 worker ID。Ait 不直接写原生日志。
- 进程 writer 由父 coordinator 独占。正在启动及已经运行的同一 worker 不得重入恢复；
  取消启动会释放保留名额并终止外部进程。只读观测不创建另一写入者。
- 每个父会话最多 32 个 worker 记录、8 个工具调用，通道、答案预览和 RPC 结果均有预算。
  完整 worker 历史仍由 OMP 日志保存。结果超预算返回明确错误，可按单个 worker 查询。
- 取消 director、退出模式或关闭父 owner 会取消工具等待并终止 worker 进程组，清除审批；
  已记录的 worker 标为 canceled，保留只读历史。重启后模式关闭，worker 不自动续跑；
  用户重新进入模式后可对已记录 worker 追问，复用其经验证的原生句柄。
- Worker 执行或启动期间拒绝改变父配置；模式启用期间拒绝原地历史切换。
  Worker 空闲时的配置变更先关闭旧 writer，再重新协商模式，新 worker 使用新配置；
  已有 worker 的后续回合保留其创建时配置。

### BTW Branch 的公开接口补充（2026-11-10）

OMP 18.17.0 已公开 BTW 查询和追问 RPC，但 TUI 的 `branchFromBtw` 未对 RPC 或扩展上下文开放。
Ait 增加 `ait-btw-branch-v1` 小型扩展命令，通过公开的 `ctx.newSession` 初始化回调和
`ctx.switchSession` 完成提升。扩展没有 OMP SDK import，不复制或改写安装目录。

- Rust owner 从原生 `get_btw_history` 读取完整问答；客户端仅传话题 ID，不能传问答、路径或任意日志。
- 遵循原生单轮提升限制：回答完成且非空，主会话和 BTW 均空闲，没有队列或后台工作；
  topic 的 leaf 与当前 leaf 相同。多轮追问话题保留在 BTW 历史，不丢弃前面的轮次来提升最后一条。
  当前接口不接受带图片的历史话题。前端只提供符合可见条件的分支入口，服务端再次校验。
- 外部 OMP 在同一进程中快照当前原生 branch entries（含模型与 compaction），初始化新会话，
  追加完整 user/assistant 问答，保存并重新加载该会话。原日志保持不变，新 header 的 parentSession
  指向原日志；不会把问答作为新 prompt 发送模型。提升文本无新增推理费用，写入零 usage。
- 这里使用公开 new/switch hooks，不冒充未公开的 TUI `reason: btw` hook。
  发生加载错误时尝试恢复原会话；无论成功失败，Ait 都重新读取真实句柄和历史，再完成操作回执。
  UI 在成功后收起 BTW；错误保留答案和草稿。原生完成与带随机 requestId 的扩展确认必须同时收到。
- 主会话忙时拒绝分支，不为等待分支而中止任务。BTW 的 follow up 继续使用 `btw(recordId, question)`，
  只取消 BTW 使用 `btw_cancel(recordId)`。扩展命令不出现在用户命令菜单中。

## 后果与验证范围

发布包没有 OMP/Bun 资源或网络安装步骤。适配器需要维护公开扩展/RPC 的兼容性，
不再维护修改版 OMP。目录读取、原生预设和侧问答仍使用同一外部可执行文件。

定向测试覆盖公开模式切换、真实外部 worker 工具执行与结果回传、审批、追问、恢复、
父会话核验及取消。模型服务使用回环地址上的模拟 API，不代表真实上游模型的规划质量；
完整桌面签名包和外部版本矩阵仍需各自验收。历史私有运行时测试不计入当前证据。

本次可复现范围见[外部 OMP Vibe 验证报告](../../reports/providers/omp-external-vibe-2026-11-09.md)。
