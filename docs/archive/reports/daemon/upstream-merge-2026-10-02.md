# 上游同步与 OpenCode 兼容验证

日期：2026-10-02。目标分支：`feat/opencode-native-plugin`。

## 合并范围

- 本地父提交：`49c4e3035d86a631c3f27176875b6964b973a5d2`。
- 原仓库：`https://github.com/ait-app/ait`，默认分支 `main`。
- 上游父提交：`3ab58e57cd55a9140b4a6ef7f11bcdf1774e2116`，新增 33 个提交。
- 合并上游完整历史，版本随上游更新至 0.0.13。保留 OpenCode Provider 与 Nix 开发环境。
- 新增 `upstream` remote，后续可用 `git fetch upstream` 同步。

## 冲突解决

1. `bins/server/src/host.rs` 同时注册 OpenCode 与 DeepSeek Harness，保留各自二进制配置和 DeepSeek 图片目录。
2. 进程测试同时隔离两种 Provider 的可执行文件，并纳入两套真实 server 回归。
3. daemon 状态断言覆盖四种 Provider，按目录的稳定排序验证。两边将总数从 2 改成 3 的相同修改被 Git 自动合并，但总数实际为 4；完整进程回归发现并修正两条诊断断言。
4. Agent 创建沿用已注册 adapter 校验，不恢复固定白名单；保留上游 idle 时清理 `activeTurn` 的更新。
5. 文档索引保留双方内容。OpenCode 决策编号从 ADR-067 改为
   [ADR-072](https://github.com/KirisameLonnet/ait/blob/819a0c41c836b7745275f1bd64833343173abf4b/docs/decisions/adr-072-opencode-server-provider.md)，避免与上游 DeepSeek ADR-067 重号。

本轮未新增领域边界；上游边界决策与既有 OpenCode 决策一并保留。

## 验证

验证环境：macOS arm64，Nix flake，Rust 1.98.1。Rust 使用锁定依赖和默认 features。

| 检查                                             | 结果                                               |
| ------------------------------------------------ | -------------------------------------------------- |
| Rustfmt / 全 targets Clippy（`-D warnings`）     | 通过；修正断言后复验                               |
| `cargo build --workspace --locked --offline`     | 通过                                               |
| Rust 完整测试与覆盖率                            | 1595 passed、0 failed、3 ignored；覆盖率指标见下文 |
| 版本一致性、本地包链接                           | 通过，0.0.13                                       |
| 桌面 / 移动发布脚本                              | 12 / 16 passed                                     |
| SDK 依赖与桌面主进程构建、Desktop / App 类型检查 | 通过                                               |
| Protocol                                         | 首次 747 passed、2 超时；失败文件复跑 9/9 passed   |
| Desktop                                          | 373 passed、13 skipped                             |
| App 相关回归                                     | 166 passed、1 skipped                              |
| Client SDK                                       | 216 passed                                         |
| 56 个改动 JS/TS 文件的 oxfmt / oxlint            | 通过，0 lint warnings/errors                       |

Protocol 两项测试动态加载约 4.7 MB 的生成校验器，首次和单文件默认 5 秒运行均超时。
单 worker、60 秒超时复跑该文件耗时 9.59 秒，9 项均通过；未修改测试断言或业务实现。
这不表示默认超时配置已通过，低资源环境仍可能需要增加测试运行超时。

```sh
nix develop --command cargo fmt --all --check
nix develop --command cargo clippy --workspace --all-targets --locked --offline -- -D warnings
nix develop --command cargo build --workspace --locked --offline
nix develop --command cargo test --workspace --locked --offline -- --test-threads=1
npm ci --no-audit --no-fund
```

以下 npm 命令均通过 `nix develop --command` 使用 Node 24：

```sh
npm run verify:local-packages
npm run verify:release
npm run test:release
npm run test:mobile-release
npm run build:desktop-main
npm run test --workspace=@ait/protocol
npm exec --workspace=@ait/protocol -- vitest run src/messages.providers-snapshot.test.ts --maxWorkers=1 --testTimeout=60000
npm run typecheck --workspace=@ait/desktop --workspace=@ait/app
npm test --workspace=@ait/desktop -- --maxWorkers=2
npm run test --workspace=@ait/app -- src/i18n/resources.test.ts src/runtime/rust-server maestro/support native-release-version.test.ts src/hooks/sidebar-workspaces-view-model.test.ts src/hooks/use-acp-provider-catalog.test.ts src/screens/settings/browser-tools-config.test.ts
npm run test --workspace=@ait/client -- --maxWorkers=2
```

未执行图形界面 E2E、打包发布、移动端原生构建和真实在线模型调用；
上述 Provider 进程测试使用离线协议夹具。

## 外部复测重点

此前测试机器人验证到 `e996f9c`。三个确认的问题已在本次合并父提交
`49c4e3035d86a631c3f27176875b6964b973a5d2` 修复，本次保留这些实现及回归：

- OpenCode 1.18.33 的空文本占位、空增量和 `message.part.delta` 不应触发执行失败。
- 用户输入不得以助手消息重复显示；核查流式输出和重启后的持久时间线。
- 桌面创建 Workspace 并提交初始输入时，根 `agent` 和 `creation.agent` 的
  `persistence.nativeHandle` 均应为字符串，界面应正常接收创建响应并进入会话。

历史真实 CLI / 正式 schema 验证见 [修复报告](../providers/opencode-native-streaming.md)。
建议机器人在最终合并提交上重测两版 OpenCode 的完整 Electron 创建、多轮、
工具允许/拒绝、取消后续输入和多次重启；这些 GUI/在线场景未在本轮执行。

## Test coverage

完整 workspace 单元/集成测试和 LLVM 测量分别均为 **1595 passed、0 failed、3 ignored**。
普通测试另包含 11 个文档测试目标（均无 doctest）。三项跳过为已安装 Claude 探测、Claude 在线推理与 Codex 在线推理。

| 测量范围          | 行覆盖率 | 已覆盖 / 总行数 |
| ----------------- | -------: | --------------: |
| Rust workspace    |   91.76% |   46474 / 50650 |
| server-provider   |   92.37% |   20832 / 22552 |
| server-filesystem |   90.17% |   10547 / 11697 |
| OpenCode 生产插件 |   85.28% |     2474 / 2901 |

测量对象为上列两个父提交合并并修正诊断断言后的源码。逐文件覆盖率、完整源码 SHA-256、日志指纹与命令见
[共享覆盖率制品](upstream-merge-2026-10-02-coverage.json)。默认 features、cargo-llvm-cov 默认测试源码过滤，无自定义文件排除；doctest 未插桩。
本轮未测量前端行覆盖率；前端通过数量单独列在验证表中。

相较本分支父提交的[历史制品](../providers/opencode-streaming-coverage.json)，workspace 从 91.52%（44143 / 48235）变为 91.76%，上升 0.24 个百分点。未在本轮重测父提交；变化包含全部上游更新，不能仅归因于冲突修复。

```sh
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-upstream-coverage.json
```

HTML 位于 `target/llvm-cov/html/index.html`，共享审阅使用上述 JSON。运行数据库、profiles、原始 RPC 和凭据不提交。
OpenCode 启动、异常关闭和部分工具/推理投影仍有未覆盖路径；后续补充这些异常分支以及真实 Electron、在线模型和其他平台复测。
