# DSH 提问回答后的空确认响应修复

## 现场与根因

2026-10-21 12:51:41（Asia/Shanghai），本机 Ait 在用户回答 DSH 的
`ask_user_question` 后，将该轮记为 `Provider execution failed`，同时
`agent.permission.resolve.request` 返回 `AgentIo`。

只读核对本机 daemon 日志、Ait timeline 与 DSH 原生 journal 后，确认用户的自定义文字回答
已进入原生 `tool/result`，且 `isError: false`。错误发生在 Ait 处理确认响应时。

本机官方 `@deepseek-ai/dsh` 版本为 `0.2.0-rc.2`。其 `dsh-api-gateway` 的
`dispatchRpc` 对 `$events/result` 返回 `{ ok: true, value: undefined }`，JSON 序列化后为：

```json
{
  "type": "server-response",
  "rpcId": "<request-id>",
  "result": { "ok": true }
}
```

Ait 原先要求所有成功 RPC 都具有 `result.value`，因此把已送达的回答误判为失败。
测试 Host 则错误地模拟为 `value: null`，此前测试没有覆盖真实响应。

## 修复

- 仅对 `$events/result` 接受成功响应中省略的 `value`，在适配器内规范化为 JSON null。
- 保持请求身份、HTTP 状态、错误响应及其他有返回值 RPC 的校验。
- 将测试 Host 的默认确认响应改为真实的省略字段形式，使现有审批、拒绝、问题与委托测试
  共用正确契约。
- 新增自定义多字节文字回答的会话回归，验证回答只提交一次、待处理问题清除、后续助手输出
  与轮次完成正常到达。
- 新增默认忽略的本机安装验证：临时 `DSH_HOME`，只提交一个未知事件的无操作结果，
  通过真实 `$events/result` 确认响应验证兼容性，不创建会话、不请求模型。

本次没有改变领域边界。未改写现场会话、凭据或数据库，也未替换正在运行的桌面 daemon。
运行中的 Ait 仍需加载包含本次修复的构建后才能使用新行为。

## 验证

首次复现基线：`e3d4a7de0c2650d0c0cf7490bcf2679d263e63a4` 加本次修改。
PR 验证基线：main `e7268bfdc2c7027529dbe74b674eac57ee49d349` 加本次修改。
平台：macOS / aarch64。以下命令禁用当前沙箱无法访问的 sccache；涉及网络的测试仅监听回环。

回归先红后绿：

```sh
RUSTC_WRAPPER= cargo test --locked --offline -p provider --lib local::deepseek_harness::native::tests::questions::custom_question_answer_with_void_acknowledgement_allows_turn_to_continue -- --exact
```

修复前在提交回答后稳定返回 `Failed`；修复后通过。

定向测试：

```sh
RUSTC_WRAPPER= cargo test --locked --offline -p provider --lib local::deepseek_harness
```

89 项通过，7 项安装环境相关测试默认忽略。
没有运行完整 Rust workspace 测试。

真实安装验证：

```sh
AIT_TEST_DSH_BIN=/opt/homebrew/bin/dsh RUSTC_WRAPPER= cargo test --locked --offline -p provider --lib local::deepseek_harness::native::tests::questions::installed_host_accepts_void_event_result_acknowledgement_without_model_execution -- --exact --ignored
```

1 项通过。此项验证原生确认响应，不代表重新执行了现场用户的模型轮次。

格式、lint 与文档检查均通过：

```sh
cargo fmt --all --check
RUSTC_WRAPPER= cargo build --locked --offline --workspace
RUSTC_WRAPPER= cargo clippy --locked --offline -p provider --all-targets -- -D warnings
RUSTC_WRAPPER= cargo clippy --locked --offline --workspace --all-targets -- -D warnings
node_modules/.bin/oxfmt --check docs/reports/README.md docs/reports/providers/dsh-event-result-acknowledgement-2026-10-21.md
npm run check:docs
git diff --check
```

文档检查验证了 418 篇 Markdown 的本地链接。workspace 构建与 Clippy 仅编译和静态检查，
没有执行完整 workspace 测试。

## Test coverage

未测量（not measured）：本次未请求覆盖率测量，因此没有覆盖率百分比、行数或覆盖率产物。
以上测试执行结果独立于覆盖率；如后续需要测量，应仅对 provider 中 DSH 相关测试运行
`cargo llvm-cov`。现场会话在已更新桌面构建中的继续执行仍待验证。
