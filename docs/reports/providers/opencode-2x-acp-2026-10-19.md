# OpenCode 2.x ACP 复查

- 日期：2026-10-19。
- 基线：`44aa000569f36d854a8e83d72ca26ff4c9eb7bc5`，加本次 2.x 复查修改。
- 环境：Linux x86_64、默认 Cargo features。
- 决策：[ADR-121](../../decisions/providers/adr-121-acp-turn-history-reconciliation.md)、
  [ADR-159](../../decisions/providers/adr-159-opencode-acp-native-observation.md)。

## 版本与隔离

核对官方 npm `@opencode/cli` 的 `latest` 为 2.0.32，另测当前支持下限 2.0.25。
从官方 `@opencode/cli-linux-x64` 下载对应包，核对 registry 的 SHA-512 integrity 后
解包到 `/tmp/ait-opencode-<version>/package/bin/opencode`，两版均实际执行 `--version`。
未替换系统的 1.18.4，没有升级用户数据库、改写用户配置或消耗外部模型额度。

每项真实 CLI 测试独立创建 XDG、配置、数据库和 loopback 模型；保持测试默认并行度。
2.x 使用 ACP 自有 private server，未启用 1.18 的 HTTP/SSE 补充观测。

## 修复与已验证行为

| 项目                  | 结果                                                                                                                                                                                                    |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/compact` 归入上一轮 | 2.0.32 真实复现：live marker 后的权威历史使用了上一条用户输入的轮次。现将独立原生 compaction ID 关联本次接纳输入，持久保存，并与普通输入共用 4096 项预算。恢复时先记录已有身份，避免重新关联旧 marker。 |
| 自动压缩              | 1.18.4 可在首轮回答后压缩；2.x 本测试在下一次输入执行前压缩。两种时机均验证 live loading/completed、摘要进入后续模型输入、回放、恢复与继续。                                                            |
| 手动压缩              | 2.x 要求模型摘要至少包含一个模板标题。夹具改为提供 `## Objective`；未放宽 native 摘要校验。验证独立压缩轮次、完整回放和恢复后继续。                                                                     |
| 认证失败回归          | 原断言仍期待旧版原文，实际已有安全分类提示。断言改为精确验证现有 Authentication 类别的固定消息，不改变产品诊断。                                                                                        |
| 输出长度限制          | 2.0.32 原生插入 synthetic 续写，下一步正常 stop 后完成；2.0.25 直接返回 max_tokens，Ait 显示输出上限提示。两种情况都验证后续用户继续；Ait 不重发输入。                                                  |
| 模型拒绝、取消        | 验证拒绝不显示正常完成、错误提示后允许用户继续；取消后使用上一有效模型步骤的原生用量，并能再次输入。                                                                                                    |

2.x 的 native context `used` 来自 `TokenUsage.total(last)`，包括输入、输出、reasoning 和缓存项，
与 1.18 的 input + cache.read 不同。测试中第一步 input 12,345、output 5，取消第二步后
应为 12,350 / 32,000。Ait 保持原生口径，不累计多个步骤的窗口使用量。

常规矩阵还覆盖：多轮与历史导入／恢复、五个独立会话、模型／effort／mode 与 system prompt
重连、64 个配置模型、四个目录并发冷启动、目录刷新、图片／附件、原生问答、shell、
多行审批、115 KB 文件 read/edit、once/always/拒绝审批、无工具辅助摘要及临时会话清理。

## 仍存在的原生 ACP 缺口

2.0.32 的 [turn.ts](https://github.com/anomalyco/opencode/blob/v2.0.32/packages/cli/src/acp/turn.ts)
仍只在 settle 时调用 `sendUsageUpdate`；每步 `session.step.ended` 只更新内部 state。
因此长工具轮次期间没有实时窗口更新；
[replay.ts](https://github.com/anomalyco/opencode/blob/v2.0.32/packages/cli/src/acp/replay.ts)
也不在 `session/load` 重放用量。Ait 已保存的 lastUsage 可以由现有应用恢复链路保留，
但不能仅凭该 ACP 接口给全新导入的会话补造当前计数。

[translate.ts](https://github.com/anomalyco/opencode/blob/v2.0.32/packages/cli/src/acp/translate.ts)
将 length 映射为 max_tokens、content-filter 映射为 refusal，其他 finish 默认 end_turn，
包括 unknown。上游不把原生 unknown 暴露到 ACP 返回，Ait 无法只凭该返回区分它与正常结束。
将 1.18 的 unknown-finish 诊断用例用于 2.0.32 对照，实际得到 Completed，
以 `unknown finish hidden` 失败；这是有意复现的原生缺口，不计入通过数量。
不将缺失字段推断成错误，不自动提交 Continue。

[catalog.ts](https://github.com/anomalyco/opencode/blob/v2.0.32/packages/cli/src/acp/catalog.ts)
的冷目录等待仍以插件库存非空为条件，没有“所有异步插件模型注册完成”的 ACP 屏障。
这与 [既有慢插件复现](opencode-2.0.25-compatibility-2026-10-13.md#慢插件的独立原生限制)
一致；此次源码核对不能替代针对用户实际插件的运行验证。

这些缺口没有通过把 1.18 的 HTTP schema 套到 2.x 来隐藏。2.x 已有原生 ACP 压缩 marker，
继续直接消费；控制、文本、工具、审批、取消和历史始终走 ACP。

## 验证

- 最终 OpenCode 离线相关回归：83 passed、27 ignored。
- 真实 2.0.25：22 passed；真实 2.0.32：22 项支持性用例通过，最后修改的两项
  stop/cancel 用例另行复核 2 passed。初轮的旧认证原文断言已修正，未改变产品诊断。
- 真实 1.18.4 复核：用量／unknown finish 3 passed，自动／手动压缩 2 passed。
- provider 全 targets 严格 Clippy、Rustfmt、8 个 Markdown 格式和 389 个文档链接检查通过。
- Workspace build 通过，保留未改动 filesystem 的 `fetch_update` 弃用警告。
  全 workspace 严格 Clippy 的基线阻挡同前次报告：未改动 host-link / model 测试的
  8 个 `assert_is_empty` 错误。本次不扩大修复范围。
- 2.0.32 的 unknown-finish 诊断有意复现 1 项失败，不计入上述支持性用例数量。
  未将预期缺口测试改成宽松成功断言，也没有声称长轮次实时用量或 unknown finish 已在 2.x 修好。

单独的 unknown-finish 诊断（2.0.32 预期复现失败，不属于该版本支持性矩阵）：

```sh
AIT_TEST_OPENCODE_BIN=/tmp/ait-opencode-2.0.32/package/bin/opencode cargo test --locked -p provider --lib installed_legacy_acp_exposes_unknown_model_finish_and_preserves_user_continuation -- --ignored
```

支持性矩阵与检查：

```sh
cargo test --locked -p provider --lib local::opencode
AIT_TEST_OPENCODE_BIN=/tmp/ait-opencode-2.0.32/package/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed -- --ignored --skip installed_acp_without_native_forms --skip installed_acp_broken_v2 --skip installed::usage::
AIT_TEST_OPENCODE_BIN=/tmp/ait-opencode-2.0.32/package/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed::v2 -- --ignored
AIT_TEST_OPENCODE_BIN=/tmp/ait-opencode-2.0.25/package/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed -- --ignored --skip installed_acp_without_native_forms --skip installed_acp_broken_v2 --skip installed::usage::
AIT_TEST_OPENCODE_BIN=/usr/local/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed::compaction -- --ignored
AIT_TEST_OPENCODE_BIN=/usr/local/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed::usage -- --ignored
cargo fmt --all --check
cargo clippy --locked -p provider --all-targets -- -D warnings
cargo build --locked --workspace
node scripts/check-docs.mjs
```

没有运行全 workspace 测试，未测真实付费模型、用户插件组合、其他平台和客户端手动 UI。
旧 2.0.0–2.0.24 的拒绝边界未改变；本次没有下载这些版本重跑原生测试。

官方协议说明：[OpenCode 2.x ACP](https://dev.opencode.ai/v2/docs/cli/acp/)。

## Test coverage

**not measured**：用户未请求覆盖率测量。上面的定向测试数量不代表覆盖率。
如后续要求测量，用 `cargo llvm-cov` 限定 provider 的 OpenCode 回归过滤器并生成当前提交报告。
本次没有生成 coverage 产物。
