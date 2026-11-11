---
id: ADR-236
status: accepted
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
4. **身份与迟到回复**：刷新以发起时的 Host client 身份为准，`isCurrent` + host-runtime
   订阅在连接更换时中止在途刷新并取消其 query；目录读写仍按 serverId+cwd 键控，
   generation/revision 单调保护不变。定时任务表单额外校验当前 selectedServerId/workingDir
   后才应用目录。数据刷新使用真实 AbortSignal 贯穿 GET、materialize 与等待，不使用空控制器。
5. **未知持久值保值**：remembered 模型在 provider loading/error 时仍显示原始 ID；
   请求的 thinking option 与已保存模型即使不在当前目录也保持原值（可见未解析），
   不被静默默认化。已知 mode 目录（含无 mode 的 provider）证明过期偏好不可用时才丢弃，
   目录缺失时保留原值；OpenCode 不再用目录默认值替换已保存模型。

## 后果

所有实际入口共享一次刷新与同一份缓存目录；失败不再清空可用候选，也不创建 Agent 或模型调用。
UI/公共 SDK/daemon wire 字段不变：旧 Host 走既有降级，旧客户端不受影响；这是客户端行为变化，
不是协议接口变化。定向验收覆盖并发单飞、loading 不追加、失败保留/重试、迟到连接隔离与
未知值保持，以及真实浏览器目录刷新冒烟。
