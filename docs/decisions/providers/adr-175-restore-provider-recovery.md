# ADR-175：独立恢复 Provider 错误详情与重启收尾

- 状态：Accepted
- 日期：2026-10-20
- 部分取代：ADR-170 的后端回滚状态；客户端诊断仍保持回滚。
- 延续：ADR-032、ADR-144、ADR-162。

## 决策

从 #406 拆出仅包含 Rust daemon／Provider 的恢复，基于主线 `e3f0dac7`。
客户端 UI、SDK 诊断采集、移动端入口与 CI 改动另放 Draft PR。

1. 恢复 Codex 有界脱敏的原生失败说明、数字 HTTP／RPC 状态和已知重试结果，
   使用现有 `failure_message()`、Timeline、`lastError` 和 `turn_failed.error`。
   不新增协议字段，已有客户端可以显示相同错误消息。
2. 在执行 worker 接纳请求之前收尾无本进程 live writer 的持久 running。
   固定中断提示先于状态提交；写盘失败阻止启动，重试保持幂等。
   保留原生句柄、配置、用量和 metadata，继续由用户显式恢复，不自动重发输入。
3. 保留主线 #376 的 OpenCode 上下文／用量／压缩实现，以及 #414 的原生会话导入。
   OpenCode 观测模块只调整测试归属和 HTTP 夹具端口交接，不修改上下文计算。
4. 不恢复客户端诊断采集、弹窗、复制按钮或移动端终端／会话入口，不改动发布工作流。

## 验证与 Test coverage

定向回归覆盖 Codex 原生失败、脱敏与重试，恢复事务失败、幂等和 live writer 保留，
OpenCode 用量／压缩以及真实 daemon 强制退出与显式继续。
最新结果与命令见[后端拆分验证](../../reports/daemon/provider-recovery-split-2026-10-20.md)。

**not measured**：用户未请求覆盖率。如需测量，使用 `cargo llvm-cov` 限定上述 Provider
模块及 daemon OpenCode 进程用例；完整 workspace 测试和测量仍需明确请求。
