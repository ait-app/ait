# ADR-097：Google Play 内部测试手动发布

状态：Accepted（2026-10-07）

## 背景

[ADR-080](adr-080-standalone-android-release.md) 的 Android 入口提供 GitHub APK 下载。
Play 开发者账号和 Ait 应用就绪后，需要生成 AAB 并向内部测试轨道提交。仓库原有本地
版本规则会使同一源码的重复构建复用 `versionCode`，而 Google Play 要求上传版本号唯一。

## 决策

- 新增独立手动入口 `release-android-play.yml`，选择 `build-only`（默认）、
  `internal-draft` 或 `internal`。空标签使用所选分支触发时的提交；稳定标签使用标签源码。
  不依赖桌面 Release，没有推送触发器或 production 选项。
- `production-play` profile 继承现有 `ait` 项目身份，生成 store AAB，沿用 EAS 托管
  上传签名和 APK 构建的 medium 资源及 Hermes 内存配置。CI 冻结凭据。
- 仅 Play runner 临时切换到 EAS remote 版本来源并自动递增 Android `versionCode`。
  `versionName` 保持仓库版本；APK、iOS 与仓库全局 local 配置继续沿用现有版本规则。
- 工作流版本的发布工具安装到 runner 临时目录，避免混入标签源码的 EAS 上传归档。
  工具为标签源码补充 Play profile，并核对源码与工具配置中的公开 EAS 项目身份一致。
- 提交前验证 EAS build 的项目、平台、profile、源码提交和版本，再用固定版本及校验和
  的 bundletool 检查 AAB，验证上传签名、包名、实际 `versionCode` 和非 debug 属性。
  保存未改动的 AAB、构建信息和 SHA-256 校验和到 GitHub artifact。
- 草稿和发布 profile 固定为 `dev.ait.mobile` 的 `internal` 轨道。验证完成后只提交本次
  build ID，不使用 `--latest` 或构建时自动提交；全局串行执行该工作流。
- Play API 服务账号 JSON 保存于 EAS 项目凭据，GitHub 复用 `EXPO_TOKEN`。Play 权限
  限于 Ait 应用的查看信息和发布测试轨道；测试者名单由 Console 人工维护。

## 后果

维护者可先下载 AAB 或上传草稿检查，再发布给内部测试者；重复构建无需修改产品版本。
既有 GitHub APK 发布入口继续独立运行。Play App Signing 与 EAS 上传签名是不同职责，
使用 Google 应用签名密钥时，Play 安装包与 GitHub APK 可能无法相互覆盖更新。

新个人账号的 production 权限仍取决于 Google 的封闭测试要求，本工作流只覆盖内部测试。
首次配置、发布、版本重试及真机验收见
[Google Play 内部测试与 CI 发布](../../operations/google-play-internal-testing.md)。
