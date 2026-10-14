# Ait 0.1.3 启动拥堵与草稿会话查询诊断

日期：2026-10-14 至 15。平台：macOS arm64。安装版：Ait 0.1.3，Codex CLI 0.162.1。
初始复现基线：`1acb7f839fee856afaa7f9e95a5ea90dd56ad1ee`。
PR 合入 `main` 的 `63ac7f7e` 后复核，验证包含本 PR 修改。
工作区版本随 `main` 更新为 0.1.3，本次未修改发布版本或替换正在运行的安装包。
相关 Timeline、Chat outline 和原生诊断源码与本地 `v0.1.3` 标签一致。

## 安装版证据

- UTC 15:19:18.900 至 15:19:28.305，9 个 Timeline 请求 ID 共被拒绝 44 次。
  全部为 `agent.timeline.get.request`，日志明确标记
  `resource=capability_queue`、`capability_lane=3`、`queue_capacity=16`。
  同一请求 ID 被重试多次，启动窗口后没有同类 Timeline 拒绝。
  这是连接 Provider 队列的启动突发，与已接纳任务等待共享额度的机制不同。
- 两次 `AgentNotFound` 均为 `agent.timeline.list_prompts.request`，目标为
  `tab_<id>`。草稿提交期间的 `AgentStreamView` 使用标签页身份展示本地消息，
  但其 Chat outline 在没有 Timeline epoch 时仍查询原生提示词索引。
  这条路径可复现，不能将这两次错误直接解释为已存在 Agent 的恢复失败。
- 原先四条 Codex 告警只有 `source=stderr` 与通用 timeout 分类，缺少原生组件和请求身份。
  原诊断报告显示 Codex 日志目录不可用。只读检查本机 Codex `logs_2.sqlite` 后，
  发现同一时段存在 `codex_models_manager::manager` 的
  `failed to refresh available models: request timed out`；这些原生记录不能逐一对应
  那四条 daemon 告警，因此不认定四条告警的根因已经全部确认。

## 修复与复现

1. 客户端所有经 DirectorySynchronizer 发出的 Timeline 页面读取继续共享同一去重入口，
   每个 DaemonClient 最多同时发送 4 个请求，其余按 FIFO 等待。成功或失败均释放名额；
   排队中的相同请求仍合并，不同连接相互独立。API 的 16 项队列、共享任务额度和
   SDK 重试策略保持原值。
2. Chat outline 等拿到有效 Timeline epoch 后再查询提示词索引。草稿仍可展示本地消息；
   已创建会话加载时间线后正常查询，已有 epoch 校验和迟到响应隔离继续生效。
3. Codex 模型目录刷新失败仍记录安全告警，标记
   `source=model_catalog`、`operation=model/list`。这类发现过程的失败不再写入
   当前轮次的执行失败原因，也不覆盖已有执行错误。普通执行超时仍按原方式记录。

修复前，模拟繁忙 Provider 队列的 9 个并发读取发生拒绝，缺少 epoch 的草稿仍调用
提示词索引；两项测试均失败。修复后全部读取完成、零拒绝，并在 epoch 到达后才查询索引。
回放原生模型目录 timeout 日志的测试也在旧实现失败，新实现保留告警但不制造轮次失败。

原生隔离探针仅发送 `initialize` 和 `model/list`，没有发送 `turn/start` 或模型任务。
两项分别约 0.76 秒和 5.79 秒完成，没有复现 timeout。首次探针因默认文件系统沙箱限制
未能启动原生服务，放行只读诊断进程后成功；这次限制不计为产品故障。

## 验证

- 前端：4 个相关测试文件、58 项测试通过，涵盖并发去重、排队、失败释放、连接隔离、
  缓存后校验、草稿索引与目录同步。
- Rust：Provider 原生诊断、Codex 失败分类与相关 AgentManager 的 13 项定向测试通过。
- 移动端 TypeScript 类型检查、修改文件 ESLint / Oxfmt、Rust fmt、Provider 严格 Clippy
  与本地文档链接检查通过。
- daemon 开发构建通过，没有编译警告。

主要命令：

```sh
npm run test --workspace=@ait/mobile -- src/timeline/fetch-agent-timeline-once.test.ts src/timeline/fetch-agent-timeline-page.test.ts src/agent-stream/chat-outline/use-chat-outline.test.tsx src/runtime/directory-sync/index.test.ts
npm run typecheck --workspace=@ait/mobile
cargo test --offline --locked -p provider local::diagnostics::tests -- --nocapture
cargo nextest run --offline --locked -p provider -E 'test(local::diagnostics::tests) | test(capacity_failure_is_classified_for_one_manager_owned_terminal_notice) | test(service::agent_manager::tests::usage)'
cargo clippy --offline --locked -p provider --all-targets -- -D warnings
cargo fmt --all --check
npm run check:docs
```

没有重启运行中的用户会话或修改本地数据。源码修复需进入新的客户端与 daemon 构建；
尚未进行安装包重启后的真实启动验收，也未验证远程主机或移动设备。
本次没有修改模块依赖、原生会话所有权或 API 接纳边界。

## Test coverage

未测量覆盖率：用户未请求统计。上述测试通过数量不代表行覆盖率；没有运行完整 Rust
Workspace 测试或覆盖率。若后续请求测量，限定 Provider 诊断与上述客户端相关行为，
记录修复提交、实际范围和可审阅产物。
