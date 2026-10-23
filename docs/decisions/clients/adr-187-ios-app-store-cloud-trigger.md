# ADR-187：GitHub 手动入口用专用标签触发 iOS App Store 构建

状态：Accepted（2026-10-24），扩展 [ADR-183](adr-183-ios-xcode-cloud-release.md) 的发布入口；
原有 `Default` 内部测试工作流保留。

维护者需要从 GitHub Actions 启动可用于 App Store 的 iOS 构建，并自动提交外部 TestFlight。
新增 `Release iOS to App Store` 手动工作流，校验选定源码的稳定版本、Cargo/npm 锁文件版本
和已提交的 iOS 原生版本，再推送指向该提交的唯一 `ios-app-store/` 标签。Xcode Cloud 的
同名工作流监听此前缀，以 App Store Connect 分发方式归档，并通过内部和外部 TestFlight
post-action 上传和分发。正式 App Store 审核提交和审核通过后的发布仍手动操作。

选择 Git 标签而非 App Store Connect API 是为了复用已连接的 Xcode Cloud GitHub App，
不在 GitHub 存储 Apple 私钥、签名证书或登录凭据。GitHub 只使用本次运行的 `GITHUB_TOKEN`
创建专用标签和读取 checks；归档、签名、上传及递增构建号仍由 Apple 完成。Cloud 的手动
入口保留，常规分支推送、PR、桌面 `vX.Y.Z` 标签不会自动触发这条发布工作流。

GitHub 等待同一源码的新 Cloud Build/Archive checks，忽略历史构建，并在失败或超时时报错。
这些 checks 不证明上传、Beta 审核或测试组已可用，后置步骤以 Cloud 和 TestFlight 为准。
每次运行和重试使用新标签，不移动既有标签；取消 GitHub 运行不会取消已启动的 Cloud 构建。
不确定远端状态时先检查 Cloud，避免重复归档。操作和云端配置见
[App Store iOS 手动发布](../../operations/releasing.md#app-store-ios-手动发布)。
