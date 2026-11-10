# Ait 0.1.16 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.15 来源为 `c22e359cbe32a272ce60a3e946f2ff4c37ee40f6`。
本次 main 基线 `8238ce07e0503ad7cc831ab56a55c0708436bc46`，纳入 #454、#446、#460、#462、#461、#459、#435、#463、#465、#466、#467 等桌面相关变更。
发布 Linux、macOS、Windows 桌面、Linux/macOS 独立 daemon，以及后续 COS、官网和服务端下载入口。
按用户要求，GitHub 正式 Release 完成后暂停，确认后继续；不执行 iOS 或 Android 构建与商店提交。
移动端工作流的历史变更不构成本次移动发布范围。

## 更新内容

- 新增 Windows x64 NSIS 安装包和 ZIP；不附带 daemon，连接远程 Host 使用，当前未配置 Windows 签名。
- 工作区浏览器统一经过所选 Workspace daemon 出站，改进远端浏览器配置处理。
- 在线服务显示登录名和账号到期时间，已保存 Host 隔离到当前登录账号并清理失效记录。
- 上游分支失效时仍可创建工作区，重新选择相同目标时保留创建错误。
- 多个 Provider 保留原生失败原因；Antigravity 后台调用增加缓存鉴权保护。
- Claude 后台任务通知显示为工具记录，改进中文翻译及深浅色品牌资源。

## 发布准备与验证

15 个 Cargo 包、7 个活跃 npm 包、锁文件、PKGBUILD 和 CHANGELOG 同步为 0.1.16。
版本准备不修改第三方依赖或 Rust 实现。

- `npm ci`、`npm run verify:release -- v0.1.16`、发布工具 67 项测试通过。
- 源码版本元数据相关 35 项测试、桌面主进程构建、桌面与共享 UI 类型检查通过；这些检查不构建或提交移动端。
- 本地 Rust 测试与覆盖率依据 AGENTS.md 跳过；[基线 main CI](https://github.com/ait-app/ait/actions/runs/40033697291)通过。
- 正式公开前必须确认准备 PR、最终 main CI、三平台发布构建、macOS 签名公证、Linux 安装与生命周期、Windows 打包客户端启动门禁成功。
- 发布产物逐项核对附件、SHA256SUMS、GitHub digest、更新 SHA-512、两个 BUILD-INFO 来源。

iOS 原生版本元数据已同步：`Info.plist` 为 0.1.16 / 1016999，锁定 CocoaPods 1.16.2 重新生成本地 ExpoTwoWayAudio 0.1.16 的 `Podfile.lock` 并通过 `pod install --deployment`。
该同步仅更新两个原生版本文件；预生成产生的图标变化已清理，未构建或提交移动端。

Windows 仅支持远程 Host，无内置 daemon，安装包未签名。未验证真实外部 Provider、生产 Hub/APNs/FCM 投递或移动端真机安装。

## Test coverage

本次重新测量三个 JavaScript 发布工具，覆盖 **285/312 行（91.35%）**、分支 **91/96（94.79%）**、函数 **22/23（95.65%）**。
测量基线 `8238ce07e0503ad7cc831ab56a55c0708436bc46`，准备工作区包含版本与文档修改；最终合并提交需通过源码指纹核对等价关系。
范围为 `release-assets.mjs`、`release-layout.mjs` 和 `release-version.mjs`，不代表 Rust、TypeScript 或整个产品覆盖率。

```sh
node --experimental-test-coverage \
  --test-coverage-include=scripts/release-version.mjs \
  --test-coverage-include=scripts/release-assets.mjs \
  --test-coverage-include=scripts/release-layout.mjs \
  --test scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs \
  scripts/release-layout.test.mjs scripts/release-workflow.test.mjs \
  scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs \
  scripts/edit-windows-executable.test.cjs
```

三个文件行覆盖率依次为 93.43%、67.86%、100%。未覆盖资源脚本 43、129–135、228–237 行，布局脚本 20–28 行，主要为 CLI 和失败路径。
0.1.15 相同三文件范围为 265/292 行（90.75%），本次增加 Windows 支持后目标源码已变化；覆盖行和总行各增加 20，比例增加约 0.59 个百分点。
这只比较定向发布工具，不能推算产品覆盖率。[JSON 证据](release-0.1.16-coverage.json)记录精确范围、命令和计数。
