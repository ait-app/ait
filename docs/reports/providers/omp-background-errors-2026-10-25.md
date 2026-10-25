# OMP 后台进度冲突与失败原因透传

- 日期：2026-10-25
- 基线：`7c824d0a`，分支 `fix/omp-failure-propagation` 的工作树改动。
- 平台：Linux x86_64；验证使用 Rust 1.98.1。
- 决策：[ADR-185](../../decisions/providers/adr-185-native-provider-failure-details.md)。

## 最近的失败

本机 daemon journal、OMP 原生日志／会话和 Ait 持久时间线记录了下列失败，时间为 Asia/Shanghai：

| 时间                | 会话与后台任务         | 观察                                                                                  |
| ------------------- | ---------------------- | ------------------------------------------------------------------------------------- |
| 2026-10-24 19:39:59 | gpu-unlock，`bg_8`     | 后台构建完成回传期间会话被 dispose，OMP 报告投递队列在 dispatch 前被清空。            |
| 2026-10-25 09:58:25 | gpu-unlock，`bg_6`     | 原工具已返回 Backgrounded；另一个 CUDA 工具运行时会话被 dispose，后台投递队列被清空。 |
| 2026-10-25 10:01:58 | ccswitch 更新，`bg_15` | curl 下载英文 OCR 数据超时；async-result 写入后会话被 dispose，新的模型请求被中止。   |

OMP 原生 bash 在后台化后继续调用原工具的 onUpdate；agent-loop 将这些事件继续发送给宿主。
Ait 已把 Backgrounded toolResult 保存为完成的工具条目，之后仍对同一 identity 写 progress。
时间线拒绝对已完成条目追加 progress，共享协调层将冲突转换为失败并关闭原生连接。

离线协议夹具复现了这条路径：修复前，后台任务的迟到进度使最后状态变成 Error，
错误文字恰为 `Provider execution failed`。原生日志没有保存完整 stdout，不能声称
已逐帧还原上述历史失败；这条可复现路径与三次失败的原生后台任务生命周期吻合。
curl 超时属于工具结果，不能据此判定模型 Provider 失败。

10:20:12 等时间另有 `set_fast_mode` 拒绝和临时连接关闭日志。当前代码为每个模型启动
临时会话探测 Fast 支持，拒绝代表不支持该能力；这些记录没有对应的新会话失败终态。

## 改动与继续执行

- 已完成工具的迟到进度不再写入原工具条目，OMP 原进程、后台结果投递及 agent loop 继续运行。
- 回归覆盖 agent_end／尚未 settled 的 prompt_result、迟到进度、async-result、再次 agent_start
  和最终 assistant 回复，也覆盖后台回传时另一个前台工具仍在执行；随后在同一原生会话中继续下一轮输入。
- 明确的原生失败保留原因、HTTP 状态和脱敏；rpc_frame_error 不再丢弃其错误说明。
- OMP 投影失败显示缺失字段、未匹配工具结果、无效用量等固定原因；stdout 的 JSON／分块／
  帧错误分别归因。后续清理不会覆盖原生错误说明。
- 原生错误和投影错误均经完整管理器链路验证：错误终态、持久快照及重开时间线使用同一说明。

## 验证与 Test coverage

使用上述基线加工作树改动，provider crate 的相关定向回归 172 通过、3 个依原配置跳过。
跳过用例需要已安装 OMP 或显式付费推理配置。补充前后台并行场景后，另行重跑 OMP
诊断链路 5 个用例全部通过。格式、provider 全目标 Clippy（`-D warnings`）及 455 份文档的本地链接检查通过。

```sh
export PATH=/var/tmp/ait-rust-1.98.1/bin:$PATH
cargo test -p provider --lib --locked -- local::omp diagnostics \
  service::agent_manager::tests local::opencode::tests::termination \
  local::deepseek_harness::native::tests::session --format=terse
cargo test -p provider --lib --locked local::omp::tests::diagnostics -- --format=terse
cargo fmt --all --check
cargo clippy -p provider --all-targets --locked -- -D warnings
npm run check:docs
```

未运行完整 Rust workspace 测试，未调用付费模型，也未重放历史 Shell 命令或修改正在运行的 daemon。

**not measured**：用户未请求行覆盖率。后续需要时使用 `cargo llvm-cov` 限定 OMP、
共享诊断和管理器错误链路；本次没有生成覆盖率工件，不将通过用例数当作覆盖率。
