---
id: ADR-234
status: accepted
date: 2026-11-11
amends:
  - clients/adr-097-google-play-internal-release.md
  - clients/adr-175-android-play-gradle-release.md
paths:
  - .github/workflows
  - apps/mobile
---

# ADR-234：稳定版 Release 自动提交 Google Play 内部测试

## 背景

Android 商店发布由独立手动工作流 `release-android-play.yml` 完成，每次版本发布后都要再
触发一次，并选择 `build-only`、`internal-draft` 或 `internal`。维护者决定每个稳定版都直接
进入内部测试，手动步骤只增加遗漏风险。

## 决策

- 删除 `release-android-play.yml`，构建与提交步骤作为 `release.yml` 的 `android-play` job，
  沿用 ADR-175 的 Gradle 构建、校验和上传机制。
- 每个稳定标签都以 `completed` 状态提交 `internal` 轨道，取消 `build-only` 和
  `internal-draft`，以及 ADR-097 中不依赖推送触发、可构建任意分支提交的入口。
  预发布标签不提交 Play，稳定标签校验保持不变。
- `android-play` 不进入 GitHub Release 的 `needs`，并设置 `continue-on-error`。
  `ait-release` 编排在整次运行结论不为成功时停止，Play 失败不能阻塞桌面发布。
- 上传继续通过 job 级并发组全局串行，并以 `queue: max` 保留排队中的稳定版。AAB artifact 改名为 `android-play-aab`，
  避免被 GitHub Release job 的 `ait-*` 下载模式收入发布附件。
- `versionCode` 公式不变，运行编号改取自 `release.yml`。切换时其计数已高于旧工作流，
  迁移基数无需调整。

## 后果

发布者不再需要额外操作，内部测试者随稳定版收到更新。Play 失败只体现在该 job 及运行
摘要中，整次运行仍为成功，需要主动查看。`ait-release` 的等待时间包含 Android 构建。
手动重跑旧标签也会上传，并因 `versionCode` 更高而成为内部测试的最新版本。不再能从未打
标签的分支提交构建到 Play。操作步骤见
[Google Play 内部测试与 CI 发布](../../operations/google-play-internal-testing.md)。
