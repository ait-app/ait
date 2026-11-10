# OpenCode 上游 PR 整合验证

日期：2026-10-03。合并父提交：OpenCode `81dd9e5c7b8728f7edd28071f3a648cb9c729df4` 与
上游 `6ce645040c0ef03d9d677ce9224c0fa9cdcb028b`（Ait 0.0.14）。

## 整合结果

- 接纳上游 daemon、crate 和客户端 workspace 重命名，OpenCode 位于 `crates/provider`，
  组装和进程回归位于 `bins/daemon`。更新 Rust 引用、Python 夹具和 TypeScript 契约夹具路径。
- Cargo.lock 保留上游依赖版本，仅加入 OpenCode 所需依赖；创建入口沿用注册适配器校验。
- Codex 适配与共享 Timeline 保持上游实现；OpenCode 原生协议、用户先于助手增量及可选工具字段修复保留。
- 按上游分类整理报告，移除已退役的 Harness 原型说明，保留其 Git 历史；OpenCode 决策使用唯一编号
  [ADR-074](../../../decisions/providers/adr-074-opencode-native-provider.md)。

## 验证

整合后的检查全部通过：

- Rust：1,617 passed / 0 failed / 3 ignored，包含 88 项 daemon 进程测试与 43 项 OpenCode 回归。
- workspace fmt、Clippy（`-D warnings`）和 build 通过。
- protocol：763 项通过；desktop：375 项通过、13 项按原配置跳过；mobile 相关测试：52 项通过。
- 发布脚本测试 15 项、移动发布脚本测试 16 项通过；本地包与发布布局验证、desktop main 构建、
  desktop/mobile TypeScript typecheck、改动文件格式/lint 和文档链接检查通过。
- desktop 跳过项依赖本机集成配置或特定平台；不计作真实桌面交互验收。

主要命令（通过 `nix develop --command` 执行）：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline -- --test-threads=1
npm ci --offline
npm run check:docs
npm run verify:local-packages
npm run verify:release
npm run test:release
npm run test:mobile-release
npm run build:desktop-main
npm run test --workspace=@ait/protocol
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
npm test --workspace=@ait/desktop
npm run test --workspace=@ait/mobile -- src/i18n/resources.test.ts src/runtime/rust-server maestro/support native-release-version.test.ts
```

## Test coverage

已测量合并后的源码，精确内容由[覆盖率制品](opencode-upstream-pr-coverage.json)中的父提交与源码 SHA-256 标识。
测量后仅更新报告和制品。范围为完整 workspace、默认 features、macOS aarch64、Rust 1.98.1、
cargo-llvm-cov 0.8.7，采用默认文件过滤，无额外排除，不包含 doctest 插桩。

| 范围      | 已覆盖 / 总行数 | 行覆盖率 | 相比 `81dd9e5` |
| --------- | --------------: | -------: | -------------: |
| workspace | 46,540 / 50,678 |   91.83% | +0.01 个百分点 |
| provider  | 20,901 / 22,592 |   92.52% |  0.00 个百分点 |
| OpenCode  |   2,564 / 2,963 |   86.53% |  0.00 个百分点 |

基线来自[合并前测量](opencode-history-order-and-tools-coverage.json)，未在本轮重测，差异包含上游全部改动。
上游另有[仓库审计测量](../daemon/repository-audit-2026-10-03-coverage.json)：workspace 43,974 / 47,707（92.18%）、
provider 18,334 / 19,625（93.42%），平台、编译器及 features 相同，但 cargo-llvm-cov 为 0.8.4，作为历史参考保留。

```sh
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-opencode-upstream-coverage.json
```

覆盖率运行也通过 1,617 项测试；3 项依赖安装/认证的 Claude discovery、Claude/Codex 在线推理用例保持 ignored。
本地 HTML 为 `target/llvm-cov/html/index.html`；共享制品包含逐文件统计、源码与日志指纹。
重要缺口包括原生 OpenCode 进程异常退出、部分 reasoning/限额/异常历史分支，以及真实发行版、GUI 与其他平台。

## 桌面验收限制

本次自动化验证不等同于真实 OpenCode 发行版和 Electron 桌面验收。测试机器人仍需在最终提交上使用
1.18.33 / 1.14.46 复验多轮顺序、旧历史恢复、工具允许/拒绝后的刷新、取消后继续、重启和工作区切换。
1.14.46 原生 CLI 缺少实时增量的既有表现不属于 Ait 适配器新增承诺。
