# Ait 0.1.9 发布说明与验证

## 发布基线

上一正式版为 `v0.1.8`，从其当前 GitHub 附件 `BUILD-INFO.json` 重新核对实际源码为
`2111128f2d765475a6b7e28da308f4a38775a0c3`；附件 SHA-256 为
`bf6f76412d5c8efe1afb733b978d1e05dea8263465e3fad07e8b4af4231cfcc8`。
本次纳入 main 至 `4740234ad0c9878993b601358f2639f1c901bb03` 的功能与修复。
不可变标签 `v0.1.9`；最终源码、工作流提交和运行链接以本次 `BUILD-INFO.json` 为准。

## 更新内容

- OpenCode 原生状态观察、历史和压缩跨 ACP 版本兼容；Pi 减少重复状态请求，Claude 忽略重放的权限回复。
- 恢复被中断的会话，复制完整错误诊断，保留 Codex 上游错误细节，修复本地 daemon 启动竞态。
- 移动端支持拍照附件，改进键盘、侧栏拖动、终端和分屏触控；已知主机更早重连，账户 Channel 断线保留主机与离线记录。
- 改进模型名和模式标签显示；文件标签路径操作与 diff 保持一致，工作区重置清理未跟踪文件，文件响应预算预留传输空间。
- 清理旧 relay 与 workspace service proxy URL；新增 Linux deb/rpm，修复系统依赖和移除钩子；nightly 附带独立 daemon 归档。
- macOS 新包内嵌 `https://aitapp.cn/api/v1/updates/macos/` generic feed，禁用多段 Range；旧客户端安装本版后迁移。Linux 保留 GitHub 更新源。

## 发布准备与验证

- 15 个 Cargo 包、7 个 npm 包、两个锁文件、根 PKGBUILD 与 CHANGELOG 同步为 0.1.9；第三方锁定依赖未改变。
- `npm ci`、`npm run verify:release -- v0.1.9`、`npm run test:release`（60 项）与 `npm run test:mobile-release`（41 项）。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`。
- 30 个本次变更的共享界面单元测试文件（396 项）；共享协议和客户端 12 文件 379 项；更新器 4 文件 50 项；Chromium 19 文件 154 项。
- 发布准备中实际发现桌面 account-state 测试仍断言丢弃离线主机；单独运行可复现失败。按已合入 PR #386 的契约修正为持久化所有有 server ID 的记录，并用不同 server ID 验证仅在线可路由主机可用；现有真实 HostRuntime 集成覆盖该行为，生产逻辑未修改。
- 文档检查、变更文件 Oxfmt 与 `git diff --check`。本机 Node 26 的共享界面单元测试使用 `NODE_OPTIONS=--no-experimental-webstorage`；保持原并行度，正式 CI 使用 Node 24。

正式发布前等待 PR/main CI、Linux 与 macOS 两平台构建、签名、公证和成品 daemon 生命周期门禁；
下载核对完整附件、SHA256SUMS、自动更新元数据、不可变标签及 BUILD-INFO，再将实测证据写入 Release Notes。
之后先上传验证 COS 版本目录，最后更新根 JSON，同步并验证官网与服务端动态入口。
桌面结束后使用同一标签发布 iOS TestFlight、Android APK 和 Google Play Internal Testing，分别记录真实构建与提交结果。
本报告记录发布准备证据，最终运行、公开产物及上线结果由 Release Notes 与跨仓库交付记录补充。

## Test coverage

本次在 macOS arm64 实测 **provider、filesystem、domain、metadata 四个变更 crate，默认 features**。
LLVM 源码行覆盖率 **89.3178%（47484/53163）**；这是所选 Rust crate 的测量，不能称为整个产品或 workspace 覆盖率。
测量提交 `b2a9b905f660b2a7a7db5190f7133897d78d8e46`，**1649 通过、0 失败、39 跳过**，nextest 默认并行度。
共享 [JSON 证据](release-0.1.9-coverage.json) 记录 315 文件的行计数、LCOV 零执行行、1190 项 Rust/Cargo 构建输入指纹及全部忽略测试名称。

| Crate      | 覆盖行/总行 | 行覆盖率 |
| ---------- | ----------- | -------- |
| provider   | 32225/35148 | 91.6837% |
| filesystem | 9145/10090  | 90.6343% |
| domain     | 543/556     | 97.6619% |
| metadata   | 5571/7369   | 75.6005% |

精确命令：

```sh
export CARGO_TARGET_DIR=target/release-0.1.9-coverage
cargo llvm-cov clean --workspace --locked --offline
cargo llvm-cov nextest -p provider -p filesystem -p domain -p metadata --locked --offline --html --status-level all
cargo llvm-cov report -p provider -p filesystem -p domain -p metadata --json --output-path /tmp/ait-release-0.1.9-coverage-raw.json
cargo llvm-cov report -p provider -p filesystem -p domain -p metadata --lcov --output-path /tmp/ait-release-0.1.9-coverage.lcov
```

`clean --workspace` 仅清理本次独立目录的 instrumentation；没有手动运行整个 Rust workspace 测试。
无额外文件排除，沿用 cargo-llvm-cov 默认测试源码过滤。HTML 位于
`target/release-0.1.9-coverage/llvm-cov/html/index.html`，共享证据以仓库 JSON 为准。
测量后仅增加文档、证据与 TypeScript 测试修正；最终交付前比较全部 Rust/Cargo 输入指纹。

39 个忽略测试需要外部工具、模型服务或平台环境，本次不解除忽略。主要未覆盖行为包括原生 provider
启动、传输失败与取消分支，以及 filesystem/metadata 的外部系统分支；精确未覆盖行见 JSON。
Linux、Windows、doctest、实际模型服务、真机 iOS/Android 不在本次本地测量内。
JavaScript/TypeScript 行覆盖率未测，其通过数量单独报告。上一版仅测 provider，范围不同，
不计算百分点变化；移动端云构建与商店提交成功也不能代替真机验收。
