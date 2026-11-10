# Ait 0.1.19 发布说明与验证

## 发布范围与来源

上一正式版 0.1.18 的当前 BUILD-INFO 实际来源为 `0042835a17c5664d1c851ead26d6b769d5e8926c`（同版本重建，原标签仍在 0ca6889a）。
本次 main 基线 `c4666b8048a60f4b104efaa78f663832b8b3521e`，纳入 #488、#489、#492。
发布 Linux/macOS/Windows 桌面、两个独立 daemon 及当前工作流附带的网页客户端镜像。GitHub 正式 Release 与公开下载验证完成后暂停，确认后继续 COS、官网和服务端下载入口。
本次不触发 iOS/Android 构建或商店提交；iOS 工程版本仅作为发布源码元数据同步。

## 更新内容

- Codex 大历史按页投影，保留加载错误，避免历史加载失败或取消后错误地继续接收新消息。
- 语音诊断记录关闭发起来源和媒体退出原因，便于区分 Provider 关闭和传输故障。
- Android 发布工具从 AAB 直接提取 R8 映射到文件并流式校验，修复超过 64 MiB 时的输出缓冲失败；本次不执行移动端发布。

## 验证

15 个 Cargo 包、7 个 npm 包、锁文件、CHANGELOG 与 PKGBUILD 的 \_ait_version 同步至 0.1.19。第三方依赖和 Rust 实现没有因版本准备而改动。

- npm ci、verify:release、75 项发布工具测试、36 项原生/移动端发布元数据测试、桌面主进程构建和桌面/共享 UI 类型检查通过。
- 生产 Expo prebuild 与 CocoaPods 1.16.2 / Bundler 4.0.21 实际生成 iOS 0.1.19 / 1019999，严格 pod install --deployment 通过；仅保留版本字段与本地 Pod 版本/checksum，无关生成资源已恢复。
- [功能基线 CI](https://github.com/ait-app/ait/actions/runs/40318651354)已成功。最终合并提交 CI、三平台打包、签名公证与成品检查作为公开前门禁；结果记录在 Release Notes。
- 版本准备未修改 Rust 实现，本地 Rust 测试/覆盖率按 AGENTS.md 跳过。未进行真实模型语音、大历史会话、移动设备或本机已安装 Electron 升级验收。

Windows 包未签名、不含 daemon，需要远程 Host。网页客户端镜像发布与生产部署分别记录。

## Test coverage

使用 Node v22.23.4 实测四个 JavaScript 版本/发布工具：**326/353 行（92.35%）**，分支 **109/114（95.61%）**，函数 **25/26（96.15%）**。

```sh
node --experimental-test-coverage --test-coverage-include=scripts/release-version.mjs --test-coverage-include=scripts/release-assets.mjs --test-coverage-include=scripts/release-layout.mjs --test-coverage-include=scripts/source-version.cjs --test scripts/source-version.test.cjs scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs scripts/release-layout.test.mjs scripts/release-workflow.test.mjs scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs scripts/edit-windows-executable.test.cjs
```

逐文件行覆盖率：release-assets.mjs 93.73%、release-layout.mjs 67.86%、release-version.mjs 100.00%、source-version.cjs 100.00%。与 0.1.18 的四文件范围相同，目标源码指纹一致：True。本次重新测量，Node patch 版本单独记录；不据此推断产品覆盖率或 Rust 变更覆盖。
精确命令、逐文件计数和源码指纹见 [JSON 证据](release-0.1.19-coverage.json)，最终发布提交需再次匹配这些指纹。
