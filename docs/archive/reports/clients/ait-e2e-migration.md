# Ait E2E、运行路径与旧功能清理

日期：2026-09-29。下列本地测试与覆盖率对应代码版本 `a933517`，基于 `main` 的 `c03c42d`。合并准备时另同步 `main` 的 `49f17b8`（Codex 大图片输出修复），仅需手工合并文档目录；历史覆盖率不重新标为新版本测量。

## 结果

- App E2E 只启动隔离的 Rust `server`，使用独立数据目录、端口、Bearer token、真实 server UUID、生产 Rust transport 和浏览器认证票据。
- 默认 Provider 使用 Rust 测试里的离线 Codex stdio fixture；真实 Provider 需显式启用。日志写入隔离目录的 `server.log`。
- App 移除 relay 连接、扫码/配对、插件运行时、路由、UI 扩展与直接依赖；旧持久化 relay 连接和插件面板在恢复时丢弃。
- 移除不再发行的 Node CLI 安装入口、shim、passthrough，以及依赖旧 Node server/插件/relay 的桌面测试启动器。现有 Ait GUI 和 Rust 生命周期测试保留。
- 工作区归属使用 server 元数据；项目配置默认 `ait.json`，兼容读取旧文件，保存不覆盖旧文件。原生附件缓存、搜索目录与克隆临时目录采用 Ait 名称。
- 项目版权、作者和维护者更新为 Necokeine；上游及第三方许可归属保留。真实仓库目录、共享 SDK 包名、IPC 与持久化存储键未统一改名。

## 验证

提交准备阶段已在独立工作树运行以下检查；开发阶段的定向结果由本轮检查补充。

- `RUST_TEST_THREADS=4 cargo test --workspace`：1,271 项通过，3 项需要本地 Claude/Codex CLI 或认证的测试忽略，0 失败。
- `cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`：通过。
- App 受影响单测：30 文件 / 511 项通过；Rust adapter、i18n 和 native release 检查：5 文件 / 73 项通过。
- protocol：65 文件 / 734 项通过；desktop：53 文件 / 373 项通过、13 项跳过。
- SDK `daemon-client-transport`：10 项通过；连接、认证与重连筛选：14 项通过、124 项未选中。
- App、desktop、client 类型检查，以及桌面构建、发布校验和 12 项发布脚本测试：通过。
- 145 个修改的 TypeScript/JavaScript 文件：`oxfmt --check` 与 `oxlint -A no-empty-pattern` 通过；该 lint 例外用于 Playwright fixture 的空解构参数。
- `E2E_AIT_SERVER_BIN="$PWD/target/debug/daemon" npm run test:e2e --workspace=@getpaseo/app -- e2e/browser/ait-server.spec.ts e2e/browser/workspace-model-restart.spec.ts e2e/browser/daemon-lifecycle.spec.ts`：6 项通过，覆盖认证、票据连接、真实 Rust Provider 离线对话、配置迁移、进程内服务重启、进程重启后工作区恢复。

需要本地监听端口的测试在允许绑定端口的环境运行。全量继承浏览器 E2E、原生移动端构建和真实在线模型 Provider 未运行。

## Test coverage

`RUST_TEST_THREADS=4 cargo llvm-cov --workspace --html`：workspace **91.60%**（35,744 / 39,021 行），`filesystem` **88.14%**（7,881 / 8,941 行），`metadata` **89.82%**（6,178 / 6,878 行）。无可比基线。范围为 `a933517` 的默认 features / macOS Rust workspace，未自定义过滤，未开启 doctest coverage，3 项依赖本地 Provider 的测试忽略。

完整测量范围、逐 crate / 修改文件计数、未覆盖行与后续验证记录在随 PR 提交的[覆盖率报告](ait-e2e-coverage.md)。HTML 已生成于 `target/llvm-cov/html/index.html`，未上传为共享 HTML。测试通过数量与行覆盖率分别统计。

## 已知范围

继承的 browser 用例仍包含旧 mock Provider、目录订阅、创建回执和旧 wire-frame 断言。本次将所有保留的启动器切换到 Ait，并建立上述可执行回归；没有声明这些继承场景已全部适配。共享 SDK 的兼容类型和包仍存在。

运行入口见 [App E2E](../../../../apps/mobile/e2e/README.md)，决策见 [ADR-061](../../../decisions/clients/adr-061-app-ait-e2e-remove-relay-plugin.md) 与 [ADR-062](../../../decisions/clients/adr-062-ait-runtime-paths.md)。

共享 client 已移除 relay/E2EE transport、静态导出和 `@getpaseo/relay` 依赖，并拒绝旧 relay URL。App 的 Metro relay 解析特例、扫码依赖和权限也已移除；独立历史包与兼容 wire 类型不属于 App 运行依赖链。
