---
id: ADR-277
status: accepted
date: 2026-11-13
amends:
  - providers/adr-091-independent-session-execution.md
  - providers/adr-156-pi-native-rpc-provider.md
paths:
  - crates/provider/core
  - crates/provider/runtime
  - crates/provider/claude
  - crates/provider/codex
  - crates/provider/omp
  - crates/provider/opencode
  - crates/provider/pi
  - packages/protocol
  - packages/client
  - apps/mobile/src/btw
  - apps/mobile/src/components/btw-window.tsx
---

# ADR-277：共享 BTW 侧会话与完整会话交接

## 背景

未合并的 PR #612 把 BTW 投影与 OMP 原生命令相连，分支操作还受原生 leaf、主会话
空闲和单轮问答限制。拆分工作留下了共享协议、Codex 和 Pi 的未发布实现，但主线没有
完整的 BTW 生命周期。客户端需要一个可关闭、连续追问、展开为完整会话的浮动对话。

## 决策

1. `provider-core` 拥有 typed `SideQuestions` port 与共享 DTO。
   `AgentClient::create_side_questions` 构造独立 runner；未实现的 adapter 返回 unavailable。
   原生命令、事件和隔离配置只由各 adapter 解析；运行时和 UI 不按 provider 名称分流。
   新 provider 接入此端口后复用全部 RPC、存储和窗口行为。原生 TUI 中存在 `/btw`
   本身不代表提供了可调用的机器接口，也不能授权把普通提示词发给主 writer。
2. `provider` 运行时拥有独立 SQLite 侧话题存储：第一次提交前冻结已保存的可读 Timeline
   上下文、provider 和模型；后续轮次复用该快照与已保存问答。侧问答不进入主输入 FIFO、
   主 Timeline 或摘要生成器。辅助进程继承 Workspace 的实际写入限制。
3. RPC 为 `agent.side_question.ask/cancel/history/transfer`。一个 Agent 同时最多一个侧轮次。
   请求 ID 与参数绑定，接受状态在持久收据中记录；不明确的接受不能自动重发、换 provider
   或降级为主会话输入。重启和断连只核对事实并收尾中断执行。
4. 运行时保存完整话题，投影单独限制数量与字节，裁剪必须标记。投影窗口不限制连续
   追问轮数，也不删除完整会话交接所需的问答；持久数据超出明确预算时返回错误。
5. 关闭浮窗仅作用于 BTW：运行中先取消辅助轮次并确认结果，再保存关闭状态。
   关闭不删除话题，显式 `/btw` 可重新打开历史。主会话仍可继续输入和执行。
6. 展开通过只读 `transfer` 取出冻结快照与完整问答，交给现有新会话草稿流程。
   不运行模型、不重新执行旧问题、不切换原 provider session、不改写原生日志。
   用户在新会话第一次发送时，按普通会话权限和配置工作，并携带已交接上下文。
7. 共享 Web、Desktop 和移动端使用同一聊天窗口；桌面浮窗保持主会话可操作，窄屏使用
   自适应对话面板。来源、追问、停止、关闭与展开有独立操作及可访问名称。

## 后果

- 主会话的单 writer 规则保留；BTW runner 是单独的辅助执行资源，不接管主 session。
- 原生 BTW 与可验证的独立无工具执行可以实现同一产品交互，但 adapter 必须明确执行策略。
  provider 之间的原生协议差异仍需适配，不以一个字符串 `/btw` 冒充自动兼容。
- BTW 使用显示上下文快照，不能宣称复制了原生隐藏推理、工具内部状态或完整原生日志。
  快照或投影发生裁剪时必须公开标记。
- 交接获得新的完整会话草稿；原侧话题与原主会话各自保留，不产生第二个原生 writer。
- 旧 host 没有这些方法时，客户端显示能力不可用；可重试的连接错误不伪装成不支持。
