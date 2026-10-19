# ADR-150：所有 Provider 的上下文压缩生命周期提示

- 状态：已接受
- 日期：2026-10-13

## 背景

聊天已有压缩完成提示和加载样式，但 Provider 未统一投影开始事件。
OpenCode 和 DSH 还会报告失败、取消和独立于推理 turn 的手动压缩。
仅按加载状态配对会把重叠的父／子会话压缩错误地合并。

## 决策

Provider 适配器拥有原生事件识别、去重和历史投影；客户端消费统一的
`compaction` 时间线条目，不根据文本或 token 数下降猜测压缩开始。

| Provider      | 原生开始信号                                                             | 原生结束信号                              |
| ------------- | ------------------------------------------------------------------------ | ----------------------------------------- |
| Codex         | `item/started` 的 `contextCompaction`                                    | 对应 `item/completed`                     |
| Claude        | `system/status` 的 `compacting`                                          | `compact_boundary`                        |
| OpenCode      | `session_info_update` 的 `_meta["opencode/compaction"].status = started` | `completed` / `failed`                    |
| DSH 原生 Host | 日志中的 `compaction/start`                                              | `compaction/end`，可携带失败              |
| DSH ACP       | `compaction_update` 的 `in_progress`                                     | `completed` / `failed` / `cancelled`      |
| Pi            | `compaction_start` 的 `manual` / `threshold` / `overflow`                | `compaction_end`，失败或取消映射 `failed` |
| Antigravity   | `stream-json` 原生 `checkpoint` 步骤的 `ACTIVE`                          | 对应步骤的 `DONE`                         |

时间线增加 `failed` 状态和可选 `compactionId`。旧条目继续支持没有 ID 的
`loading` / `completed`。DSH 老日志没有 ID 时，用开始事件的 session 和 sequence
派生身份。开始和结束保持独立、不可变的存储键；客户端按 ID 在原位置更新标记。

生命周期缓存有界，忽略重复状态和终态后的迟到开始。OpenCode 实时发送压缩条目，
其他完整消息仍由原生历史确认；重连回放相同的原生终态身份。DSH 空闲时仍消费
独立的手动压缩事件。已报告开始但执行终止且未收到成功结束的操作显示失败，
不能显示成功或持续加载。原生错误正文不进入公共压缩条目。

OpenCode 使用其 marker 扩展，不声明完整 ACP compaction summary 能力。
已验证的 OpenCode 1.18 profile 按 [ADR-159](adr-159-opencode-acp-native-observation.md)
补充同一 ACP 子进程内部的原生只读压缩事件；执行和权威文本／工具历史仍走 ACP。
Antigravity 显示 headless 协议暴露的上下文 checkpoint 生命周期。
Pi 同样处理空闲时的手动压缩；实现边界见 [ADR-156](adr-156-pi-native-rpc-provider.md)。
CLI 版本必须提供相应原生信号，客户端不能补造自动压缩开始时间。

参考：[OpenCode ACP marker](https://dev.opencode.ai/v2/docs/cli/acp/#compaction-marker)、
[DSH compaction](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/compaction.md)、
[ACP compaction](https://agentclientprotocol.com/rfds/session-compaction)、
[Antigravity headless](https://antigravity.google/docs/cli/headless)。

## 后果

所有内置 Provider 共用开始、成功和失败的展示路径，无需新增模型调用或改变压缩策略。
失败状态是共享协议扩展，daemon 与客户端应同步升级。只有完成记录的历史继续
只显示完成；不补造加载过程。缺少 ACP marker 且不在已验证原生观测 profile 内的 CLI
仍无法观察自动压缩开始，不能凭 token 下降补造压缩事件。
