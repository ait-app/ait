# Ait 0.1.18 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.17 附件 BUILD-INFO.json 实际来源为 `7dc6487eced4c185fda8f8890d4e63d7851ba5aa`。
本次 main 基线 `555eebad4694213229827ddbb97d495a9734da72`，纳入 #480、#481、#482、#483、#484。
发布 Linux、macOS、Windows 桌面与 Linux/macOS 独立 daemon，当前工作流另发布网页客户端镜像。
GitHub 正式 Release 验证完成后按用户要求暂停，确认后继续 COS、官网及服务端下载入口。
本次不执行 iOS、Android 构建或商店提交；iOS 工程版本同步是标签源码准备的一部分。

## 更新内容

- Codex 原生实时语音经 daemon 中继，统一语音转录、回合归属和恢复后的会话历史。
- 新工作区草稿可发起语音会话，改进播放、音频中断和能力选择。
- Ait ACP 桥接官方 Oh My Pi 与通用插件接口，覆盖配置、权限、历史及子 Agent 事件。
- 统一在线服务设置页，完善速度、Host 和用量相关翻译。
- nightly 与本地/AUR 源码构建使用日期和提交标识，正式版继续使用发布 SemVer。

## 准备与验证

15 个 Cargo 包、7 个活跃 npm 包、锁文件与 CHANGELOG 同步为 0.1.18。
PKGBUILD 的 `_ait_version` 更新为 0.1.18，动态 pkgver 的日期/提交格式保留。
版本准备没有修改第三方依赖或 Rust 实现。

- npm ci、verify:release v0.1.18、发布工具 75 项测试通过，0 失败、0 跳过。
- 版本元数据 35 项测试、桌面主进程构建及桌面/共享 UI 类型检查通过。
- 生产 Expo prebuild 生成 iOS 0.1.18 / 1018999；Bundler 4.0.21、CocoaPods 1.16.2 重新生成本地 ExpoTwoWayAudio 0.1.18 的锁文件，并通过 pod install --deployment。
- 原生工程只保留两个版本字段与本地 Pod 版本/checksum；无关预生成资源已恢复，未执行移动端二进制构建。
- 本地 Rust 测试与覆盖率按 AGENTS.md 跳过。[基线 CI](https://github.com/ait-app/ait/actions/runs/40176276105)、最终合并提交的适用 CI、三平台打包及成品检查均作为公开前门禁，最终结果写入 Release Notes。
- 公开前逐项核对全部附件的 SHA256SUMS、GitHub digest、实际大小、更新 SHA-512 和两个 BUILD-INFO 的来源；另验证 macOS 签名公证、Windows 无内置 daemon 及 stable displayVersion。

未进行真实 Codex 模型/实时语音、生产 ACP 插件及移动端真机验收。Windows 包未签名、需要远程 Host。
网页镜像目标 `ghcr.io/ait-app/ait-web-client:v0.1.18`；发布镜像与生产部署分别记录。

## Test coverage

本次实测四个 JavaScript 版本/发布工具：**326/353 行（92.35%）**，分支 **109/114（95.61%）**，函数 **25/26（96.15%）**。
测量基线 `555eebad4694213229827ddbb97d495a9734da72`；最终合并提交通过源文件指纹确认等价关系。
本次范围新增 source-version.cjs，且 release-assets.mjs 已随构建身份修复变更；0.1.17 的三文件 285/312 行（91.35%）不能直接与本次总比例比较。

```sh
node --experimental-test-coverage --test-coverage-include=scripts/release-version.mjs --test-coverage-include=scripts/release-assets.mjs --test-coverage-include=scripts/release-layout.mjs --test-coverage-include=scripts/source-version.cjs --test scripts/source-version.test.cjs scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs scripts/release-layout.test.mjs scripts/release-workflow.test.mjs scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs scripts/edit-windows-executable.test.cjs
```

逐文件行覆盖率：release-assets.mjs 93.73%、release-layout.mjs 67.86%、release-version.mjs 100.00%、source-version.cjs 100.00%。
未覆盖行见发布工具 CLI 与失败路径：release-assets 44、138–144、241–250，release-layout 20–28。
该数字不代表 Rust、TypeScript 或整个产品覆盖率。[JSON 证据](release-0.1.18-coverage.json)记录精确命令、范围、计数和源文件指纹。
