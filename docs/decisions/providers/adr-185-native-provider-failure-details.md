# ADR-185：各 Provider 保留原生失败原因

- 状态：Accepted
- 日期：2026-10-23
- 修订：ADR-144 的固定分类展示策略；将 ADR-162、ADR-175 的原生错误说明扩展到其他适配器。

## 背景

Claude 的 OAuth 过期等错误会出现在原生 assistant、result 或 RPC error 中，但适配器
只保留固定分类，部分字段未被读取。用户看到原生错误文本后，聊天终态仍显示
`Provider execution failed`。OpenCode 对已经识别分类的错误也会丢弃具体原因，
Pi、DSH 和 Oh My Pi 存在相同的信息丢失。

## 决策

1. 将 Codex 已有的有界原生说明提取、凭据脱敏和重试状态放到 provider 的共享
   `local::diagnostics::failure`，各适配器读取明确的错误消息字段。
   未知错误也保留安全处理后的原始原因，不要求先命中分类关键词。
2. 共享 `Diagnostics` 分别保存结构化诊断和脱敏后的说明。日志只记录原有结构化事实，
   `Debug` 不输出说明文本。共享诊断为已识别的错误补充固定的恢复建议。
3. Claude 读取错误 assistant 的 text、stream error、失败 result 的 errors/error/result 和控制 RPC 错误；
   session 保有独立诊断引用，关闭进程后仍可读取原因。共享 ACP、DSH 原生 Host、Pi 和
   Oh My Pi 读取各自 RPC／失败终态的消息。DSH 的非成功 HTTP 响应只检查最多 32 KiB 的
   JSON 错误字段，保留 HTTP 状态，绝不直接展示整个响应体。Antigravity 保留原生 result
   错误，同时延续其权限拒绝的专用操作建议。
4. 只提取错误说明，不收集 request、工具输入、responseBody 或 additionalDetails。
   JSON 字符串仅提取内层 message，无法提取时使用固定说明。保留原有 32 KiB 检查上限、
   3072 字节原因上限、UTF-8 截断标记、凭据和 URL 脱敏，最终消息不超过 4096 字节。
   任意服务文本无法保证完全不含自定义敏感信息，适配器不得扩大到任意诊断载荷。
5. 内部 `AgentSession::failure_message()` 返回 `Cow<str>`，使共享诊断能够提供已脱敏的
   拷贝，原有会话自有说明继续借用。共享协调层在关闭前捕获说明，沿用同一条持久错误
   Timeline、lastError 和 turn_failed.error，不增加 wire 字段、RPC 或 domain 依赖。
6. 新回合清除旧说明；原生重试中的错误不发布最终失败；Claude、Pi 和 Oh My Pi 后续成功的
   assistant 结果清除中间失败。不会自动重发用户输入，也不把工具执行失败当成会话失败。

## 后果

各客户端通过现有聊天错误组件展示相同的原生原因和恢复建议。断线、刷新和历史重读
继续保留错误条目。旧的泛化错误没有保存原生原因，无法追溯补全；本变更不改写原生历史。

## 验证与 Test coverage

定向测试覆盖共享提取／脱敏／长度限制、各原生协议失败、成功恢复、关闭后读取，以及
Claude OAuth 过期从原生 assistant 到终态事件、持久快照和重新打开聊天历史的完整链路。

本次定向回归：345 通过，35 个需要本机账号或已安装原生工具的用例按原有配置跳过。
未运行完整 Rust workspace 测试。验证命令：

```sh
cargo test -p provider --lib --locked -- \
  local::diagnostics local::claude::tests local::codex::tests::diagnostics \
  local::codex::transport::tests local::opencode::tests local::deepseek_harness::tests \
  local::deepseek_harness::native::tests::session local::deepseek_harness::native::http::tests \
  local::pi::tests local::omp::tests local::antigravity::tests \
  service::agent_manager::tests ports::agent_session::tests --format=terse
cargo clippy -p provider --lib --locked -- -D warnings
cargo clippy -p provider --all-targets --locked -- -D warnings -A clippy::assert-is-empty
cargo fmt --all --check
npm run check:docs
```

生产代码严格 lint 通过。本机使用 Rust 1.99.1，仓库与 CI 固定为 1.98.1；
本机测试目标包含未修改文件中的 `assert-is-empty` 告警，
all-targets 检查仅在命令行豁免这一规则，其他告警仍作为错误处理。

**not measured**：本次以受影响模块的功能回归和 lint 验证行为，未测量行覆盖率。
不运行完整 Rust workspace 测试；需要测量时使用 `cargo llvm-cov` 限定相关 Provider 用例。
