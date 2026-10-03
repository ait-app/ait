# ADR-077：Android APK 的 GitHub Release 发布

状态：Accepted（2026-10-03）

## 背景

现有 Release 工作流提供 Linux 和 macOS 安装包，需要在同一次发布中自动提供可安装的
Android APK，并沿用安装包命名、源码选择和校验和规则。当前阶段暂时不配置正式签名密钥。

## 决策

- 主工作流 `release.yml` 通过 `workflow_call` 调用 `release-android.yml`。推送版本标签或
  手动启动主发布工作流时，Android 与桌面一起构建，接收相同的标签和可选源码提交。
- GitHub runner 负责 Expo prebuild 和 Gradle release 构建。只在生成的原生项目中配置
  ABI 分包，分别输出 ARM64、ARMv7 两个完整 APK；应用身份与版本号由移动端配置管理。
- 暂时保留生成项目自带的测试签名，跳过正式 keystore 配置及重新签名，不要求 Android
  签名 Secrets。收集阶段仍验证安装包签名、生产包名、版本、不可调试标记、对齐与对应 ABI。
- APK 文件名统一由 `scripts/release-assets.mjs` 定义，沿用
  `Ait-版本-平台-架构.扩展名`，架构名为 `arm64` 和 `armv7`。
- Android 构建任务只拥有仓库读权限。现有主发布任务等待全部平台成功，合并构建产物，
  将两个 APK 作为必需附件校验，使用统一的 `SHA256SUMS` 和 `BUILD-INFO.json`，再执行发布。
- PR 通过 `android-release-check.yml` 复用同一构建工作流，以合并提交执行 dry run，只保存
  Actions artifact。正式发布和 PR 检查各自管理并发，PR 检查不取得 Release 写权限。

## 后果

维护者沿用现有发布入口即可获得 Android APK，无需单独触发 Android Action 或配置正式
签名凭据。Android 构建失败会阻止本次整套 Release 发布，可在同次运行中重跑失败任务。
当前产物使用测试签名；以后切换正式密钥需要考虑已安装应用的签名兼容性。
本决策定义工作流行为，GitHub runner 上的完整发布结果以实际运行记录为准。
操作步骤见 [Android APK 发布](../../operations/android-releases.md)。
