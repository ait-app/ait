# Diff 修复 PR 验证

日期：2026-10-02。目标分支为 main `93d6c8e8`，来源提交为 `bc14ba5a`。
本次将来源提交与最新 main 合并，保留已合入的侧边栏实现，并补齐 `ServerInfo.version`
的六处测试夹具初始化。Diff ADR 改为 [ADR-071](../../../decisions/workspace/adr-071-checkout-diff-syntax-highlighting.md)，
避免与 main 的 DeepSeek ADR 编号重复。

测量后的 Rust 源码和 Cargo 配置没有再修改；精确源码列表、SHA-256 和整体指纹见
[覆盖率证据](diff-pr-coverage.json)。整体指纹为
`43909f90821330c37eb6f751d1aca2e4a656409761cfd27170b0b85254a46c69`。

## 提交内容与验证

本次 PR 包含 Diff 行首文件头解析、服务端语法高亮及输出预算、服务版本的协议与客户端
投影，以及 GitHub 项目链接迁移到 `ait-app/ait`。此前的侧边栏改动已通过 PR #141 合入。
高亮边界及定向回归见[实施报告](diff-syntax-highlighting.md)。

平台为 macOS arm64、Rust 1.98.1、默认 features。完整 Rust 构建、Clippy、格式和空白
检查通过。完整插桩测试 **1555 通过、0 失败、3 ignored**；真实 Provider 测试维持原有
跳过配置。该覆盖率运行执行整个 workspace，没有重复计入测试数量。

```sh
cargo fmt --all --check
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-release-0.0.10-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib cargo build --workspace --locked --offline --target-dir /private/tmp/ait-diff-highlight-lint
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-release-0.0.10-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib cargo clippy --workspace --locked --offline --all-targets --target-dir /private/tmp/ait-diff-highlight-lint -- -D warnings
```

客户端相关的 6 个测试文件 **88 项通过**，覆盖 Rust transport、服务版本、更新链接与
Diff 画布。Desktop/App 类型检查、修改的 TypeScript 文件 ESLint、Oxfmt、版本一致性和
本地 workspace 依赖检查通过。SDK、语法高亮和音频包在独立 worktree 内重新构建。

```sh
npm run build:ui-deps
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
npm exec --workspace=@ait/mobile -- vitest run --project unit src/runtime/rust-daemon/messages.test.ts src/runtime/rust-daemon/transport.test.ts src/desktop/updates/desktop-updates.test.ts src/git/diff-document/model.test.ts src/git/diff-document/paint.web.test.ts src/git/diff-document/paint.native.test.ts
```

## Test coverage

使用 cargo-llvm-cov 0.8.4，范围为完整 Cargo workspace，macOS arm64、Rust 1.98.1、
默认 features、默认文件过滤，没有额外排除；不包含 TypeScript 或 doctest 插桩。

| 范围                  | 行覆盖率   | covered / total     |
| --------------------- | ---------- | ------------------- |
| Workspace             | **92.15%** | **43,989 / 47,738** |
| filesystem            | **90.15%** | **10,544 / 11,696** |
| model                 | **95.45%** | **629 / 659**       |
| api                   | **95.56%** | **1,659 / 1,736**   |
| Diff highlighter 模块 | **99.36%** | **312 / 314**       |

```sh
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-release-0.0.10-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=/private/tmp/ait-diff-pr-coverage cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
CARGO_TARGET_DIR=/private/tmp/ait-diff-pr-coverage cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-diff-pr-coverage-summary.json
```

可共享的 [JSON 证据](diff-pr-coverage.json) 包含 workspace/crate、逐文件覆盖率和源码
指纹。HTML 位于 `/private/tmp/ait-diff-pr-coverage/llvm-cov/html/index.html`，未纳入提交。
没有在同一环境重新测量目标 main，不计算与历史报告的覆盖率差值。

未注入语法解析/scope 操作失败或缓存锁中毒；没有执行 Linux/Windows、iOS/Android
设备或真实已认证 Provider 测试。本轮没有替换已安装的桌面包。
