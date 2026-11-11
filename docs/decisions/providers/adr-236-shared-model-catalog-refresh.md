---
id: ADR-236
status: proposed
date: 2026-11-11
paths:
  - apps/mobile/src/data/providers-snapshot.ts
  - apps/mobile/src/data/provider-snapshot-cache.ts
  - apps/mobile/src/hooks/use-providers-snapshot.ts
  - apps/mobile/src/provider-selection
  - apps/mobile/src/components/model-browser.tsx
  - apps/mobile/src/components/combined-model-selector.tsx
  - apps/mobile/src/components/ui/select-field.tsx
  - apps/mobile/src/composer/agent-controls
  - apps/mobile/src/agent-profiles
  - apps/mobile/src/schedules
  - apps/mobile/src/screens/settings
---

# ADR-236：共享模型目录刷新与选择保值

- 复用主线已有的 provider snapshot 作用域、内容哈希缓存与 generation/revision 单调保护；
  本 ADR 只约束客户端共享刷新的入口、失败保留与身份归属，不新增 daemon 协议字段。

## 背景

模型选择器、AgentProfile 编辑、定时任务表单、元数据生成设置与 Provider 诊断各自持有
目录入口。此前打开选择器只按 stale 判定重新 GET，不会触发原生重新发现；发现失败或
Provider 暂时不可用会清空可用候选， remembered 的模型/mode/thinking 值也会被静默替换。
多个入口同时打开时各自发起刷新，且旧 Host 连接的迟到回复可能写入新连接的目录。

## 决策

1. **唯一共享刷新入口**：`refreshProvidersOnAccess` 以 `["providersSnapshotRefresh", serverId, cwd]`
   的 query 单飞同一 Host+目录的原生刷新；并发入口加入同一在途批次，不重复下发。
   显式指定 provider 时只刷新所选 provider；未指定时刷新全部已启用 provider。
   原生正在 loading/refreshing 的 provider 不再追加刷新，由既有完成推送与 1 秒轮询收敛。
2. **入口语义**：打开选择器/下拉（composer、新建 Agent 表单、Profile 编辑、定时任务、
   元数据设置、Provider 设置重试）调用 `refetchIfStale(selectedProvider)`，在支持快照的
   已连接 Host 上走共享刷新；旧 Host 保持原有 stale GET 降级。挂载本身不扇出刷新。
3. **失败保留**：发现失败或 unavailable 且无模型时，按 provider 保留最近一次可用
   models/modes（内存快照优先，其次磁盘缓存），错误经 `entry.error` 与 hook `error`
   可见、可重试；合成结果不写回磁盘缓存，也不与 Host 内容哈希关联。重试成功后以新目录替换。
4. **身份与迟到回复**：刷新以发起时的 Host client、connectionEpoch、clientGeneration 和在线状态
   为准，`isCurrent` + host-runtime 订阅在连接更换或同一 client 重连时中止在途刷新并取消 query；
   目录读写仍按 serverId+cwd 键控，
   generation/revision 单调保护不变。定时任务表单额外校验当前 selectedServerId/workingDir
   后才应用目录。数据刷新使用真实 AbortSignal 贯穿 GET、materialize 与等待，不使用空控制器。
5. **未知持久值保值**：remembered 模型在 provider loading/error 时仍显示原始 ID；
   请求的 thinking option 与已保存模型即使不在当前目录也保持原值（可见未解析），
   不被静默默认化。已知 mode 目录（含无 mode 的 provider）证明过期偏好不可用时才丢弃，
   目录缺失时保留原值；OpenCode 不再用目录默认值替换已保存模型。

## 后果

所有实际入口共享一次刷新与同一份缓存目录；失败不再清空可用候选，也不创建 Agent 或模型调用。
公开 SDK 方法、daemon wire 和持久配置字段不变，旧 Host 走既有 stale GET 降级，旧客户端不受影响；
但客户端内部入口与交互语义发生变化，不能据此称“没有接口变化”。定向验收覆盖并发单飞、
loading 不追加、失败保留/重试、连接/epoch/cwd 迟到回复隔离与未知值保持，以及真实原生目录
变化后的浏览器刷新。

## 接口与迁移披露

| 所有者/边界      | 旧 → 新                                                                                       | 迁移与兼容                                                                                                        |
| ---------------- | --------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| UI/共享数据层    | 各入口 stale GET → `refreshProvidersOnAccess` 共享原生目录刷新                                | Host+cwd 查询下按显式 provider 批次合并并发，保留已有 content hash/generation/revision 权威性；不是另建目录状态。 |
| Hook             | `refetchIfStale(selectedProvider)` 的旧读取语义 → 已连接、能力支持时刷新选中 Provider         | 所有实际模型选择、Profile、定时任务、设置入口接入同一流程；旧 Host 仍 GET，不发送未协商 refresh。                 |
| UI 组件          | `SelectField` 没有打开回调 → 可选 `onOpen`；ModelBrowser 的已有 retry 回调也用于展开 Provider | 消费者明确连接刷新入口；不需要修改普通 SelectField 调用者，不增加编辑器或 Provider 控制接口。                     |
| UI 选择与错误    | 失败/未知值可能被清空或默认化 → 保留可用候选和原始选择，显示错误/重试                         | 只改变客户端交互行为，不重写已保存模型、mode、thinking，也不创建 Agent 或向模型发送输入。                         |
| Provider/API/SDK | 已有 snapshot GET/refresh → 同一公开接口                                                      | 无 Rust、RPC、SDK、配置 schema、原生 adapter 或二进制调用链变化；不恢复 mock Provider 或动态通用 ACP 注册。       |

浏览器验收使用当前主线已有 Codex 原生 fixture 与 `speed-models.json`，不依赖旧 mock/gemini
配置模型。实际记录及历史失败原因见 [task02](../../plans/pr612-split/task02.json)，契约与 donor
来源见 [Draft #647](https://github.com/ait-app/ait/pull/647)；本 ADR 保持 proposed 等待评审。
