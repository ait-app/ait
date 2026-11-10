---
id: ADR-237
status: accepted
date: 2026-10-31
amends:
  - providers/adr-082-deepseek-harness-native-host.md
paths:
  - crates/provider/src/local/deepseek_harness
---

# ADR-237：DSH 历史读取使用不激活 Agent 的原生接口

状态：接受。日期：2026-10-31。

## 背景

真实会话恢复的并发验收复现了原生拒绝。DSH 0.2.1-rc.4 的 `session/follow` 会在发送
冷会话快照后后台激活 Agent，因此仅收到历史快照、未发送 prompt 并不等于只读。
历史导入和列表预览使用该入口，会与实际交互恢复争抢原生会话，也可能唤醒目标 driver。

## 决策

- 交互 owner 继续使用 `session/follow` 订阅自己的会话。历史、导入和列表预览只调用
  官方 `session/list`、`session/projections`、`session/page`，不建立 follow 订阅。
- 已有会话恢复先用只读列表验证原生 ID、cwd 和普通会话身份，再建立会激活 Agent 的
  follow 订阅；不在返回快照后才发现目标目录不匹配。
- 列表验证原生身份、cwd 和子 Agent 标记。`projections` 必须提供 `sequenced` 基线，
  所有页面固定在其 `asOfSeq`；沿用连续序号校验、分页数和总字节上限。
- 迁移提示、缺少只读入口、HTTP 错误和非法基线都报告失败，不回退到会激活 Agent 的接口。
  不通过延时、收到快照后取消订阅或额外原生 writer 规避竞争。
- 只读列表没有原生创建时间。导入依照已有 `SessionHistory` 契约使用观察时间，
  保留原生最后活动时间；不把首条事件的时间伪装成创建时间，也不伪造完整原生 header。
- 列表预览优先使用原生 outline；缺失时在既有时间、页数和字节预算内读取固定事件前缀。
  目标和命令卡片继续使用同一原生 journal 投影，实时与恢复后的展示保持一致。

## 验证

Provider 回归验证只读查询与交互 writer 并发时不会增加 follow/create/prompt 请求，
并覆盖无效和迁移基线、缺失接口、空日志的 -1 cursor 及固定 cut 分页。
真实已释放会话的并发只读导入和恢复结果记录在
[DSH / OpenCode 桌面报告](../../archive/reports/providers/dsh-opencode-capability-adaptation-2026-10-30.md)。

## Test coverage

**not measured**：未请求覆盖率；定向测试和真实 CLI 验收单独记录。
