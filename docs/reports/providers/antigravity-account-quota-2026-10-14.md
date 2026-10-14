# Antigravity 已登录但额度不可用：修复验证

日期：2026-10-14。基线：`a5a0f781` 加本次工作树修改。环境：macOS arm64，默认 features。

## 根因与复现

用户已在本机 AGY 登录，但 Ait 用量返回 unavailable。
隔离 daemon 经 `provider.usage.list.request`，参数
`{"providerId":"antigravity","forceRefresh":true}` 连续两次复现：
`status=unavailable`、`windows=[]`，并附带去 Provider CLI 登录的通用提示。

同一个 `/opt/homebrew/bin/agy` 的 `models` 返回 10 个模型；
`agy --print /usage --output-format json` 成功返回两个模型组、四个 quota 窗口。
其 `status=SUCCESS`、`command.name=usage`、`num_turns=0`，conversation ID 为空，
输入/输出/总 token 都为零。由此排除本机未登录和原生接口不可用。

根因是 `AntigravityClient` 未实现 `AgentClient::usage`，Session 也未实现
`AgentSession::account_usage`，落入默认的 `Unavailable`。
通用 Manager 又将这个“不支持”解释为登录未提供额度，产生误导提示。
现有会话中的 token usage 是另一个接口，不能替代账号配额。

## 修改

- 在既有 Provider 端口内接入 AGY 内置 JSON `/usage` 命令，不引入 HTTP 认证或凭据读取。
- 主机使用该 Provider 的原生环境；活动会话使用实际启动环境及 cwd。沿用主机五分钟缓存与强制刷新，停止会话不使用主机账号兜底。
- 保留每个原生 bucket 的 ID、模型组、时间窗口、剩余比例和重置时间；已用比例为 `100 - remaining_fraction * 100`。
- 明确只接受成功的 `usage` 命令及零模型轮次；拒绝错误响应、重复 ID、非法比例、无效日期及超限结构。空额度列表报告不可用，避免提示重新登录。
- 复用既有 30 秒查询超时、2 MiB 输出上限和子进程清理；只投影显示字段，原始响应和 stderr 不进入用量卡片。

这次实现沿用 ADR-087 的官方 CLI adapter 和 ADR-119 的账号用量端口，不改变 crate 依赖或协议边界。
参考 [Google `/usage` 文档](https://antigravity.google/docs/cli/commands/usage)和
[官方仓库中对 JSON 命令的原始复现](https://github.com/google-antigravity/antigravity-cli/issues/1045)。
JSON 结构以本机原生输出核验；未假设任意旧 AGY 版本均支持该结构。

## 验证

先新增 `logged_in_antigravity_host_and_session_quotas_use_native_command`，
旧实现实际失败：期望 available，得到 unavailable。接入后通过。

定向用例覆盖模型组、5 小时/每周窗口、零剩余额度、未知窗口、空结果、非法或重复字段、
缓存和强制刷新、主机/会话环境隔离、会话 cwd、停止会话不回退到主机、
认证失败、非零退出、原生命令失败、损坏 JSON、超大输出、超时及错误文本脱敏。

验证命令：

```sh
cargo test --locked --offline -p provider --lib local::antigravity
cargo test --locked --offline -p provider --lib service::agent_manager::usage
cargo test --locked --offline -p provider --lib service::agent_execution::tests::usage
cargo test --locked --offline -p provider --lib authenticated_account_quota_without_model_execution -- --ignored
cargo clippy --locked --offline -p provider --all-targets -- -D warnings
cargo build --locked --offline --workspace
cargo fmt --all --check
npm run check:docs
```

最终结果：Antigravity 模块 37 passed、2 ignored；Manager 用量 2 passed；
执行 RPC 用量 1 passed；单独启用的真实 AGY 额度测试 1 passed（7.27 秒）。
共 41 项定向 Rust 测试通过。另一个需消耗模型轮次的安装测试未运行。

用重建的 `target/debug/daemon`、独立临时数据目录、临时认证 token 和 loopback WebSocket
重跑同一原始用量 RPC，得到 `status=available`、`windows=4`、无 RPC 错误。
临时 daemon 已退出并清理数据；未替换正在运行的桌面 daemon 或修改用户登录。
Clippy（provider all-targets，`-D warnings`）、workspace build、Rustfmt、文档格式、
358 个文档的链接检查及 diff 空白检查通过。

## Test coverage

**not measured**：本次未请求覆盖率测量。测试数量只表示执行结果，不表示覆盖率。
如后续请求测量，使用 `cargo llvm-cov` 仅运行 Antigravity 和直接相关用量测试，并发布对应报告。
未运行完整 workspace 测试、需要模型调用的原生多轮测试或 Windows/Linux 实机测试。
