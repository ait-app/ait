---
id: ADR-235
status: accepted
date: 2026-11-11
amends:
  - clients/adr-183-ios-xcode-cloud-release.md
  - clients/adr-187-ios-app-store-cloud-trigger.md
paths:
  - .github/workflows
  - apps/mobile
---

# ADR-235：稳定版 Release 通过 API 启动 iOS 内部 TestFlight 构建

## 背景

[ADR-183](adr-183-ios-xcode-cloud-release.md) 要求在 Apple 平台手动启动 `Default` 工作流，且不新增
App Store Connect API 凭据。[ADR-187](adr-187-ios-app-store-cloud-trigger.md) 的专用标签入口
因 `GITHUB_TOKEN` 缺少 `workflows` 权限而撤销。[ADR-234](adr-234-play-internal-in-release-workflow.md)
之后 Android 已随 Release 进入内部测试，iOS 仍需发版后单独操作。维护者决定两端一致。

## 决策

- `release.yml` 新增 `ios-testflight` job，由 `scripts/xcode-cloud-release.mjs` 调用
  App Store Connect API，在发布标签上启动 Xcode Cloud 的 `Default` 工作流。
- 归档、签名、上传和内部 TestFlight 分发仍由 Xcode Cloud 完成。job 确认构建已创建后即结束，
  不等待归档，也不占用 macOS runner。
- 不使用标签推送，避免 ADR-187 的权限问题。在 GitHub Secrets 新增团队 API key 的
  `APP_STORE_CONNECT_ISSUER_ID`、`APP_STORE_CONNECT_KEY_ID` 和 `APP_STORE_CONNECT_PRIVATE_KEY`，
  只暴露给启动步骤。该 key 不用于签名或上传。
- 只处理稳定标签。预发布标签跳过；带 `source_commit` 的重建也跳过，因为 Cloud 只能构建
  分支或标签，无法保证构建指定提交。
- job 启动前校验标签与仓库版本一致，因为 Cloud 的 `ci_post_clone.sh` 不检查标签。
- job 不进入 GitHub Release 的 `needs`，并设置 `continue-on-error`，启动失败不阻塞桌面发布。
  它也不等待桌面构建，不再沿用 ADR-183 中先完成桌面 Release 的顺序。
- `Release iOS to App Store` 工作流、外部 TestFlight 和 App Store 审核保持手动。

## 后果

稳定版发布后内部测试者无需额外操作即可收到 iOS 构建。GitHub 多保存一份 Apple API 私钥，
其权限应限制在启动 Xcode Cloud 构建所需范围。job 成功只代表构建已启动，归档与分发结果以
Xcode Cloud 和 TestFlight 为准。每次运行或重跑该 job 都会启动新的 Cloud 构建并消耗额度。
操作步骤见[发布指南](../../operations/releasing.md#apple-testflight-ios-内部测试)。
