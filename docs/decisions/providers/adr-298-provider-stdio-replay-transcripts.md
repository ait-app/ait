---
id: ADR-298
status: accepted
date: 2026-11-23
amends:
  - providers/adr-179-provider-owned-tests.md
paths:
  - crates/provider/native
  - crates/provider/codex
---

# ADR-298：Provider 测试回放真实 CLI 的 stdio 录制

- 范围：stdio JSON 行协议原生 CLI 的录制与回放工具、录制格式、归属和依赖边。
- 关系：扩展 [ADR-179](adr-179-provider-owned-tests.md) 第 4 条的原生程序夹具规则，
  沿用 [ADR-270](adr-270-provider-crate-family.md) 的 `provider-native` 共享工具定位。

## 背景

Adapter 测试驱动手写的假 CLI（如 `crates/provider/codex/tests/fixtures/codex_app_server.py`），
它们只反映编写者对原生协议的理解；原生 CLI 升级后帧形状变化时，这些测试仍会通过。
Codex app-server、Pi RPC 与各 ACP adapter 都通过子进程的 stdin/stdout 交换 JSON 行，
可以在进程边界录下真实帧，再在测试中回放给真实 adapter。

## 决策

1. 录制与回放共用一个 stdio 对端程序 `crates/provider/native/src/replay/stdio_peer.py`，
   由 `provider_native::replay::StdioPeer` 放到 adapter 期望的程序路径。它只在 Unix 上、
   `provider-native` 的 `test-support` feature 或其自身测试中编译；该 feature 不启用任何依赖，
   adapter crate 只以 dev 边启用它，生产构建不包含它。
2. 录制格式 `ait-stdio-replay` 第 1 版是 NDJSON：首行 `meta`（格式、版本、provider、协议、
   CLI 版本、场景、日期），之后按发生顺序记 `spawn`（第 N 次启动及其参数）、`stdin`（adapter 写给 CLI 的帧）
   与 `stdout`（CLI 写给 adapter 的帧），每行带启动序号 `process`。帧保存原样 JSON，
   格式不解释 turn、审批等语义，适用于任一 stdio JSON 行 CLI。
3. 回放按启动顺序认领录制中的进程，校验启动参数，先写出下一条 adapter 帧之前录下的全部 CLI 输出，
   再要求 adapter 的下一帧与录制相等：类型和 method 一致，除 `id` 外逐值相等，录制值 `{{any}}` 可匹配任意值；
   adapter 请求的 `id` 映射到回放响应。首个不符的帧写入进度日志并结束对端进程，
   测试用 `StdioPeer::assert_replayed` 报告不符原因，以及没有收到全部录制帧的启动。
4. 录制与回放双向使用占位符：录制把会话工作目录、私有原生目录和 adapter 版本等值写成占位符，
   回放写回测试自己的值。录制同时把本机用户名、主机名和邮箱改成固定值，把调用方列出的键（如安装 ID）
   写成 `redacted`。录制驱动真实 adapter 对真实 CLI 运行同一场景，是 adapter crate 中默认忽略、
   需显式启用的测试，使用只含凭据副本的隔离原生配置；录制文件放在所属 adapter crate 的
   `tests/fixtures/replay/`，提交前须人工复核。
5. 回放测试与普通测试一样离线运行。手写假 CLI 继续覆盖录制难以稳定触发的错误、审批、取消与恢复等场景。

## 后果与验证

Codex 的一轮文本对话录自 `codex-cli 0.169.0`，`provider-codex` 的回放测试经真实 adapter
断言 Timeline 与轮次完成，并验证 adapter 发出录制外的帧时测试失败。adapter 改变发给 CLI 的帧时，
相关录制须重新录制，这是严格比较的代价；录制需要已登录的 CLI 并产生少量模型费用。
回放不模拟 CLI 输出与 adapter 请求之间的时序竞争，也不支持并发启动的进程；需要这些场景时再扩展格式版本。
Pi 与 ACP adapter 可直接复用该格式与对端程序，尚未接入；定期对新 CLI 版本重新录制的 canary 尚无计划。

**Test coverage**：未测量；用户未请求覆盖率。定向验证为 `provider-native` 的 `replay` 测试与
`provider-codex` 的 `tests::replay` 测试。
