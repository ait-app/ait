# OpenCode 1.18.33 兼容问题核验

> 历史验证记录：下述路径、命令和指标对应所注明的源码提交；当前目录与上游整合结果见 [PR 验证](opencode-upstream-pr.md)。

核验日期：2026-09-30。基线：`3b3d5c6051465b95b5c89713ef7e39a113de4540`。

本报告记录该次离线修复，不能证明完整原生对话可用。后续真实复测发现的空文本、
用户角色与 Workspace 响应问题见 [原生流事件后续修复](opencode-native-streaming.md)。

## 结论与修复

三个问题均存在。先补充回归断言，确认原代码分别失败，再进行修复。

| 问题                     | 原因                                                                                                                                        | 修复                                                                                                                                         |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| 流式对话返回 `agent_io`  | Bridge 直接使用原生 part ID 作为 key，Timeline.progress 要求 `native:` 前缀；InvalidMessage 经 AgentManager 的 Registry 错误映射为 agent_io | 流式与最终历史使用一致的 `native:opencode:` 时间线 key，原生 messageId 保持原值                                                              |
| 交互恢复失败             | 每次恢复 PATCH permission；1.18.33 的接口将新规则追加到已有规则，之后 Ait 要求数组完全相等                                                  | v1 恢复只验证创建时的策略，不再 PATCH；兼容旧恢复遗留的完整策略副本，拒绝额外授权、顺序变化、规则修改及残缺副本；v2 保持原来的替换与精确校验 |
| Providers 页拒绝模型数据 | OpenCodeClient.discover 构造模型时遗漏必填的 provider；Catalog 原样传递 models；前端 AgentModelDefinitionSchema 要求 provider               | 每个模型定义包含 `provider: "opencode"`；真实 server 进程测试同时断言模型列表与快照中的字段                                                  |

诊断连接、已安装探测与模型响应校验属于不同检查；前两项成功不能证明模型响应有效，
也不能把本次 Providers 页异常归因于 server 未启动。

## 依据

- [1.18.33 Identifier](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/opencode/src/id/id.ts)：原生 part ID 使用 `prt_` 前缀。
- [1.18.33 SessionHttpApi.update](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts)：PATCH 使用 `Permission.merge(current.permission, payload.permission)`。
- [1.18.33 Permission.merge](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/opencode/src/permission/index.ts)：实现为规则数组 flat，保留重复规则。
- Paseo 参考提交 `5599f9e567128a1240b3b15afab28bceef9d36a5` 的 `opencode-agent.ts` 在恢复时调用 session.update；Ait 的完整数组校验使这一行为触发拒绝。
- Ait `storage/timeline/progress.rs`、`service/agent_manager/streaming.rs`、`rpc/agent_execution.rs` 构成流式错误传播路径。
- Ait `local/opencode/client.rs`、`service/provider_catalog.rs` 和 `packages/protocol/src/messages.ts` 构成模型响应与校验路径。

## Test coverage

- 修复前定向复现：19 passed、3 failed。三项失败分别为流式落库的 InvalidMessage、模型 provider 为 Null、恢复的 AgentCapabilityUnsupported。
- 修复后 OpenCode 定向测试：25 passed、0 failed。覆盖 v1/v2 流式落库及最终文本去重、发现响应、连续会话、只读历史、重复恢复、旧重复策略恢复和权限变更拒绝。
- 完整 workspace 测试：1510 passed、0 failed、3 既有在线测试 ignored；26 个完成的测试目标（含 doctest）。真实 server 进程的 OpenCode 测试通过，并在提取发现响应断言为辅助函数后单独重跑：1 passed、0 failed。
- workspace 构建、全 targets Clippy（`-D warnings`）、Rustfmt 和 `git diff --check` 均通过。
- 提交前使用 `cargo llvm-cov` 重跑完整 workspace：1510 passed、0 failed、3 ignored，并生成 HTML。
- 核验通过 1.18.33 官方源码和离线 HTTP/SSE 夹具进行。真实 server 进程测试使用原生协议夹具；未运行已登录 OpenCode 的真实模型对话，也未运行前端浏览器。

| 测量范围（LLVM summary） | 行覆盖率 | 已覆盖 / 总行数 |
| ------------------------ | -------: | --------------: |
| Rust workspace           |   91.48% |   43998 / 48094 |
| server-provider          |   92.13% |   19691 / 21372 |
| OpenCode 生产插件        |   84.49% |     2332 / 2760 |

测量对象为上述基线加本次 Rust 修复；变更源码 SHA-256、逐文件指标、未覆盖行位置、
原始结果与日志指纹见 [共享覆盖率制品](opencode-compatibility-coverage.json)。测量后只更新报告和制品。
macOS arm64，flake 的 rustc 1.98.1 / cargo-llvm-cov 0.8.7，默认 features；
使用工具默认测试源码过滤，无额外文件排除，doctest 默认未插桩，Linux/Windows 未验证。
相较同口径的基线 [迁移覆盖率制品](opencode-server-coverage.json)，workspace 上升 0.01 个百分点、
server-provider 上升 0.02 个百分点、OpenCode 插件上升 0.21 个百分点；小幅差异也可能来自测试执行的非确定性。
仍未完整覆盖原生 HTTP/SSE 异常、进程启动超时与强制关闭，以及工具/推理内容投影分支。
后续需结合真实 OpenCode 安装与模型对话验证；本轮修复的权限匹配函数已覆盖有效策略、重复副本和拒绝分支。

```sh
nix develop --command cargo test -p server-provider local::opencode --locked --offline
nix develop --command cargo test --workspace --locked --offline -- --test-threads=1
nix develop --command cargo test -p server-bin --test process opencode --locked --offline
nix develop --command cargo build --workspace --locked --offline
nix develop --command cargo clippy --workspace --all-targets --locked --offline -- -D warnings
nix develop --command cargo fmt --all --check
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-opencode-bugfix-summary.json
nix develop --command cargo llvm-cov report --lcov --output-path /private/tmp/ait-opencode-bugfix.lcov
```

本轮测试日志：`/private/tmp/ait-opencode-bugfix-workspace-tests.log`、
`/private/tmp/ait-opencode-bugfix-clippy.log`、`/private/tmp/ait-opencode-bugfix-server-test.log`、
`/private/tmp/ait-opencode-bugfix-build.log`、`/private/tmp/ait-opencode-bugfix-coverage.log`。
HTML 位于 `target/llvm-cov/html/index.html`；共享审阅使用上方 JSON。日志和运行数据不提交。
