# Provider 后端恢复拆分验证

- 日期：2026-10-20；Linux；主线基线 `e3f0dac7`。
- 来源：从 #406 拆出可独立合入的后端变更；客户端与 SDK 恢复另放 Draft。
- 决策：[ADR-175](../../decisions/providers/adr-175-restore-provider-recovery.md)。

## 范围

Codex 原生失败说明经过有界脱敏后进入既有错误消息通道，保留数字 HTTP／RPC 状态和
重试结果。daemon 启动时收尾没有 live writer 的持久 running，先写 Timeline 再提交
错误状态；写盘失败阻止启动，部分成功后重试保持幂等，原生身份、用量和配置不丢失。
恢复仍由用户显式继续，不自动重发模型输入。

保留主线 #376 的 OpenCode 上下文、用量及压缩计算和 #414 的原生会话导入。
OpenCode HTTP 代码仅调整测试模块归属与夹具 socket 交接。
此分支没有 `apps/`、`packages/` 或 `.github/` 改动，不恢复客户端采集与移动端 UI。

## 验证

在上述基线加本报告所在提交的后端恢复上，使用默认 features 和系统 Rust 1.99：

| 定向命令                                                                        | 结果                           |
| ------------------------------------------------------------------------------- | ------------------------------ |
| `cargo test --locked -p provider --lib local::codex`                            | 189 passed，1 existing ignored |
| `cargo test --locked -p provider --lib service::agent_manager::tests::recovery` | 5 passed                       |
| `cargo test --locked -p provider --lib local::diagnostics`                      | 9 passed                       |
| `cargo test --locked -p provider --lib local::opencode`                         | 84 passed，27 existing ignored |
| `cargo test --locked -p daemon --test process opencode_`                        | 5 passed                       |

进程用例使用隔离的离线原生夹具，覆盖 daemon 强制退出、OpenCode 子进程退出、首个恢复
快照、显式继续和导入会话重启；没有调用真实模型。未运行本地完整 Rust workspace 测试。

`cargo build --locked --workspace` 与 `cargo fmt --all --check` 通过，Python 夹具 AST
解析通过。构建仍有未修改 filesystem 的 `fetch_update` 弃用警告。系统 Rust 1.99 的
`cargo clippy --locked -p provider --lib -- -D warnings` 通过。Provider 全目标严格 Clippy
被主线 Pi／原生会话等测试的 13 项 `assert_is_empty` lint
阻止；CI 按仓库既有设置使用 Rust 1.98.1，未为系统新增 lint 扩大本 PR 范围。
仅在本地命令追加 `-A clippy::assert_is_empty` 后，同一 Provider 全目标检查通过；
仓库 lint 配置没有修改。

## Test coverage

**not measured**：用户未请求覆盖率，没有生成覆盖率产物。通过数量仅表示执行结果。
后续如需测量，使用 `cargo llvm-cov` 限定上述 Provider 模块和 daemon OpenCode 进程
用例；完整 workspace 测试或测量仍需明确请求。macOS、Windows 与真实原生服务未验证。
