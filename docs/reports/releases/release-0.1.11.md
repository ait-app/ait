# Ait 0.1.11 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.10 的 BUILD-INFO.json 与 GitHub digest 已重新核对，实际交付源码为
`aeafdf084d988699fe3b6ba7544bed95f5ab5cde`。本次固定 main 基线
`5c639f9520911699a44acedbb507401e6b767014`，纳入 [回滚 PR #399](https://github.com/ait-app/ait/pull/399)。
最终不可变标签为 v0.1.11，成品来源以 BUILD-INFO.json 核对。

本次发布桌面客户端、两个独立 daemon、COS、官网和服务端下载入口，以及稳定版自动 AUR。
按用户要求，不触发 iOS/Android 构建、TestFlight 或 Google Play 提交。共享 workspace 版本同步和测试不构成移动端发布。

## 更新内容

- 完整撤回 PR #381 引入的启动错误采集、统一诊断复制/导出、Codex 原生错误详情和 daemon 重启后自动收尾中断轮次。
- 恢复此前的客户端错误提示、SDK 诊断日志、Codex 失败分类和会话启动行为；背景错误记录策略随其依赖的采集器撤销。
- 保留后续独立的 Markdown、面板编辑、用量和 Setup 观测改动；无数据库迁移。
- 回滚源于 iOS 启动回归，但本次只分发桌面；移动端修复须另行明确请求构建和发布。

## 发布准备与验证

测量提交为 `68642a42cae2bfaa181565ff75d3fcd6e65f73b5`。准备改动仅含版本元数据、CHANGELOG 与发布证据。
15 个本地 Cargo 包、7 个 npm 包及锁文件和 PKGBUILD 同步至 0.1.11；第三方依赖未变化。

- `npm ci`、`npm run verify:release -- v0.1.11` 通过。
- `npm run test:release`：60 项通过；`npm run test:mobile-release`：41 项契约测试通过，未构建/发布移动应用。
- `npm run build:desktop-main`、桌面及共享界面类型检查、404 篇文档本地链接和改动文件格式检查通过。
- 9 个相关共享界面测试文件：50 项通过，无失败或跳过；SDK 12 文件 377 项通过。
- 根 AGENTS.md 要求版本/文档准备不修改 bins/crates Rust 代码时跳过本地 Rust 测试；本次未运行或测量 Rust。回滚源码 PR 与最终发布源码的适用 CI 独立核验。

正式公开前须等待准备 PR CI、适用 main 检查、Linux/macOS 构建、签名公证与成品启动门禁；
实际下载并核对 14 个桌面附件、13 项 SHA256SUMS、GitHub digest、更新 SHA-512 和 BUILD-INFO。
公开后按发布编排顺序上传 COS，版本文件全部可读后最后更新根清单；官网部署和服务端线上入口分别核验。
本报告记录准备结果，最终在线状态由 Release Notes 与跨仓库交付报告补充。

## Test coverage

在 macOS arm64、Node v26.11.1、Vitest 4.1.7 下，实际测量 66 个现存共享界面变更 TS/TSX 文件，
9 个相关 unit 文件 50 项通过、0 失败、0 跳过。V8 单元测试行覆盖率为
**1.78%（155/8678 行）**。
此分母包含大量未被定向用例挂载的页面/容器，结果只代表本次指定范围；不代表整个产品覆盖率。

[共享 JSON 证据](release-0.1.11-coverage.json)记录每个文件计数、未执行行、完整参数、测试选择与
2514 个输入 SHA-256。测量提交为 `68642a42cae2bfaa181565ff75d3fcd6e65f73b5`；正式交付前将与发布源码逐项比对指纹。
精确命令数组在证据 `commands.coverageArgv`，工作目录为 `apps/mobile`：

```sh
NODE_OPTIONS=--no-experimental-webstorage npm exec --no -- vitest run --project unit \
  --coverage --coverage.provider custom \
  --coverage.customProviderModule /tmp/ait-release-0.1.10-coverage-tools/node_modules/@vitest/coverage-v8/dist/index.js \
  --coverage.reportsDirectory /Users/necokeine/Documents/ait-realease/artifacts/releases/v0.1.11/validation/ui-coverage \
  --coverage.reporter json --coverage.reporter json-summary --coverage.reporter html \
  --reporter json --outputFile /Users/necokeine/Documents/ait-realease/artifacts/releases/v0.1.11/validation/ui-test-results.json \
  <证据列出的 66 个 --coverage.include 参数与 9 个测试路径>
```

覆盖率 provider 使用已有隔离临时安装的 4.1.7，与项目 Vitest 一致；未修改项目依赖。
选择范围为已交付 v0.1.10 至固定 main 的现存非测试 TS/TSX；删除文件、类型声明、测试源与 Vitest 默认排除项不计入。
Rust 未测量，无历史数值复用。v0.1.10 的文件/测试集合不同，无可比基线，不计算百分点变化。
原生移动启动、原生手势/动画、完整页面集成和用户安装客户端替换升级尚未验收。
