---
id: ADR-236
status: accepted
date: 2026-11-11
amends:
  - clients/adr-175-android-play-gradle-release.md
  - clients/adr-234-play-internal-in-release-workflow.md
paths:
  - .github/workflows
---

# ADR-236：GitHub Release 附带 Google 签名的 Android APK

## 背景

[ADR-175](adr-175-android-play-gradle-release.md) 移除 APK 构建后，GitHub Release 只为旧版本保留
APK 附件，站外用户需要从 Play Console 手动取得 APK。[ADR-234](adr-234-play-internal-in-release-workflow.md)
之后每个稳定版都会上传 AAB，Play 随之生成 Google 签名的 universal APK。

## 决策

- Play 接受 AAB 后，独立的 `android-apk` job 通过 Play Developer API 的 `generatedApks` 下载该
  `versionCode` 对应的 universal APK。不在 CI 中自行构建或签名 APK，GitHub 与 Play 的安装包使用同一个
  应用签名密钥。只重跑该 job 即可重试 APK，不会重新构建或再次提交 Play。
- 附加前核对包名、版本、实际 `versionCode`、ARM 架构，以及 APK 签名证书与 Play 报告的证书一致。
- `android-apk` job 在 GitHub Release job 完成后把 `Ait-VERSION-android.apk` 附加到草稿 Release，
  并更新 `SHA256SUMS`。它持有仓库写权限和 Play 服务账号，只运行工作流提交中的发布工具。
- 只修改草稿；已正式发布的 Release 不再改动附件。该 job 不阻塞 Release 和整次运行的结论。
- 修订 ADR-234 中“GitHub Release 不含 Android 产物”的结果：Release 再次附带 APK，
  ADR-234 的 AAB artifact 和 Play 提交方式不变。

## 后果

站外用户可以从 GitHub Release 下载与 Play 同签名的 APK，两种安装方式可以互相覆盖更新；
0.1.x 的旧 GitHub APK 签名不同，需要卸载重装。发布多等待 Play 的处理时间。服务账号可能需要
额外的 Play Console 权限，首次运行才能确认。`ait-release` 不镜像 APK 到 COS。
