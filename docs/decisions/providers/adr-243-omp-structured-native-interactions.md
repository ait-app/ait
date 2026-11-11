---
id: ADR-243
status: proposed
date: 2026-11-11
amends:
  - providers/adr-198-omp-interaction-progress.md
paths:
  - crates/provider
  - apps/mobile
---

# ADR-243：OMP 结构化原生交互与启动前问答

- 修订：[ADR-198](adr-198-omp-interaction-progress.md) 的表单字段与启动前交互；
  延续 [ADR-237](adr-237-omp-native-session-and-standalone-acp.md) 的原生 Session 边界。
- 状态为 proposed：本决策随 Draft PR 提交评审，不代表架构验收；既有 accepted ADR 不被改写。

## 背景

ADR-198 已把 select/confirm/input/editor/ask 全部收敛到既有 permission 事件与响应端口，
但表单仍缺少 secret、空值、预填与取消语义的显式字段：input/editor 的空字符串会被当作未回答，
dismiss 会提交空 allow 而不是取消，editor 的 native prefill 丢失，secret 输入以明文显示。
原生初始化（`connect` 的阻塞请求序列）期间插件发出的启动问题只能等待整个会话打开后才可见，
官方 18.17.0 约 30 秒即取消未回答的启动问题，用户没有机会回答。

## 决策

1. 交互继续使用既有 permission wire：`permission_requested`/`permission_resolved` 事件、
   `pendingPermissions` 快照与 `agent.permission.resolve.request`。不新增 RPC 方法、事件类型、
   providerUi 状态或通用 native_action 网关；后续 Goal/BTW/子 Agent/账户等消费者各自拥有自己的
   typed schema，不在本决策预留。
2. `input.questions[]` 增加可选 typed 字段：`secret`、`initialValue`、`allowEmpty`、
   `dismissBehavior:"deny"`、`preserveWhitespace`。OMP adapter 负责解释原生帧：
   input/editor 发布 `allowEmpty`/`dismissBehavior:"deny"`/`preserveWhitespace`，
   native `secret==true` 发布 `secret`，editor 保留 `multiline` 并把 native `prefill` 映射为
   `initialValue`；confirm 发布 `detail:{type:"unknown",input:null,output:null}`，
   以标题/消息描述确认而不是裸工具 JSON。空字符串、空白与换行按原样回传（仍拒绝 NUL、
   超长与过期/陈旧 id）；deny 仍回传 `{cancelled:true}`。
3. 新增 `omp/bootstrap.rs` provisional session：原生 open 在单一任务中执行，启动 dialog 帧经
   同一序列化 stdin `Writer` 路由给 provisional session 并立即可回答；原生会话就绪后
   `adopt_startup` 接管剩余 dialog。首条输入只排队一次（逻辑 turn id），就绪后经同一 writer
   释放；取消排队输入会关闭会话且不重放。元数据探测追加 `--no-ui`（仍加载插件，不发启动问题）。
   冷初始化预算 180 秒仅用于 ready/negotiate；连接后的控制保持原 deadline。
4. secret 值只存在于内存 Dialogs 与原生响应帧；resolution 事件沿用既有 redacted 投影，
   timeline/registry 不记录答案值。UI 对 secret 字段使用 `secureTextEntry`，按 question key
   重建输入，`initialValue` 预填并计入已回答；`dismissBehavior:"deny"` 的 dismiss 发送 deny。
5. 审批并发不依赖新调度面：`Operation::Permission` 仍在会话 lane，`agent.finish.wait` 仍是
   lane-free 读取；启动 dialog 在初始化完成前即可响应，消除“宿主等初始化、初始化等用户”的循环。

## 接口变化与迁移（完整 ledger）

| 边界                   | 旧接口/调用链                                                                                                       | 新接口/调用链                                                                                                                                                    | 迁移与兼容性                                                                                                      |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| daemon wire / 公共 SDK | `AgentPermissionRequestPayloadSchema`（`input` 为开放 record）、permission 事件、`agent.permission.resolve.request` | 不变；仅 `input.questions[]` 内新增可选字段                                                                                                                      | 旧客户端继续按文本/选项渲染；新客户端对旧宿主只是看不到可选字段。无方法注册/能力计数变化                          |
| OMP adapter 内部       | `session::open(client,spec,handle)` 直接返回 Session；`connect` 无启动路由                                          | `session::open(...,startup:Option<Sender<Frame>>)`；`bootstrap::open` 包装 create/resume；`Transport::{writer,startup,finish_startup}`、`Session::adopt_startup` | `AgentClient::create_session/resume_session` 签名不变；外部 ACP bridge 以 `None` 调用，行为不变。无新公共端口方法 |
| 原生进程参数           | 探测 `--mode rpc-ui --no-session`                                                                                   | 探测追加 `--no-ui`                                                                                                                                               | 官方 18.17.0 支持；插件仍加载，元数据读取不再触发启动问题                                                         |
| 表单 UI                | `QuestionFormQuestion` 无 secret/预填/取消语义；dismiss 对 allowEmpty 提交空 allow                                  | 新增可选 `secret/initialValue/preserveWhitespace/dismissBehavior`；secret 掩码、按题 key 重建、预填；deny-dismiss                                                | 组件 props 不变；既有 Codex/其他 provider 问题不带新字段，行为不变                                                |
| 启动输入               | 初始化完成后才能提交首条输入                                                                                        | provisional 排队一次，就绪后同一 writer 释放；取消即关闭不重放                                                                                                   | 逻辑 turn id 映射回原生 turn；重连经 `pendingPermissions` 恢复未答 dialog，不重放已取消请求                       |
| i18n                   | `message.question.*`、`common.actions.dismiss`                                                                      | 不变                                                                                                                                                             | 9 个 locale 无新增键                                                                                              |

旧宿主 + 新客户端：可选字段缺席时表单退化为既有行为（无掩码/预填），不报错。
新宿主 + 旧客户端：请求仍是既有 `kind:"question"/"tool"` permission，旧客户端可读可答。
本决策不改变 ACP 结构（task01 已完成），不新增 `_omp/*` 扩展。

## 后果

secret/空值/换行/预填/取消语义由 adapter 显式 typed 字段承载，UI 不解析 OMP 字符串；
启动问题在官方运行时约 30 秒取消窗口内可回答，纯 TUI 方法仍需插件回退（不承诺无限等待）。
provisional session 不引入第二 writer；取消与失败路径沿用既有结算。
定向验收覆盖共享夹具（Rust/protocol/UI 同源）、bootstrap 单元测试、manager 启动/重连回归与
官方 18.17.0 安装夹具（secret/empty/cancel/startup，loopback 模型，无付费推理）。
当前说明见 [OMP 架构](../../architecture/omp-acp-bridge.md) 与 [操作手册](../../operations/oh-my-pi.md)。
