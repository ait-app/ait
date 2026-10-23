# ADR-183：iOS CI 使用 Xcode Cloud 归档与内部分发

状态：Accepted（2026-10-23），取代 [ADR-070](adr-070-ios-testflight-release.md) 和
[ADR-079](adr-079-mobile-eas-builds.md) 的 iOS 发布路径。

## 背景

仓库已提交 iOS 原生工程和锁文件，Xcode Cloud 的 `ci_post_clone.sh` 能安装依赖并校验
应用版本。Ait 的 `Default` 工作流已完成 Archive 和向 `Ait Internal` 的 TestFlight
内部分发，App Store Connect 已收到构建。继续保留 GitHub Actions → EAS Build / Submit
会形成两套构建、凭据和重试入口。

## 决策

- iOS CI 直接使用 Xcode Cloud；在 Xcode 或 App Store Connect 手动启动现有 `Default`
  工作流，不通过 GitHub Actions 转发，也不新增 App Store Connect API 凭据。
- 工作流使用已提交的 `apps/mobile/ios/Ait.xcworkspace` 和 `Ait` scheme。生产身份为
  `com.necokeine.ait` / Team `SVS7GV79T9`。保留现有 Build action，并以 Archive 的
  `TestFlight (Internal Testing Only)` 和指向 `Ait Internal` 的内部测试 post-action 分发。
- 云端负责归档、签名、上传、构建号递增和 TestFlight 分发。仓库负责原生工程、依赖锁、
  版本同步和 Cloud bootstrap hook。Cloud 配置在 Apple 服务中维护，文档记录必要设置。
- 删除 GitHub EAS TestFlight 工作流、专用脚本和测试，以及上游遗留的三个 EAS 发布／审核
  工作流。Android 已迁到 GitHub Gradle，旧混合移动端 EAS 发布入口不再保留。
- 日常内部测试手动选择 `main` 提交；稳定版本在桌面 GitHub Release 完成后手动选择相同
  标签并核对 commit。不添加 push、PR、标签或定时自动发布条件。
- 不改变 Expo SDK、config plugins 和本地 prebuild；`eas.json` 的历史手动配置不再作为
  iOS CI 的入口或身份来源。当前流程不包含外部测试审核或 App Store 正式审核。

## 后果

GitHub `ios-testflight` environment 的审核、正式 Release 自动查询和 EAS 去重逻辑退出
iOS 发布路径。维护者在 Apple 平台启动构建前核对源码和发布状态，不再假定 GitHub 门禁
仍然有效。旧 environment 和 Secret 的清理由维护者确认其它使用者后处理。

Cloud 上传使用自身递增构建号，源码中的 Expo build number 仍随版本准备更新；二者用途
不同。失败重试在 Cloud 内进行，版本或锁文件错误必须修复源码，不能靠重试旧标签解决。

只执行 Build 不足以交付 TestFlight；验收必须检查 Archive、内部测试 post-action 和
App Store Connect 的处理结果。操作步骤见[发布指南](../../operations/releasing.md#apple-testflight-ios-手动发布)。
