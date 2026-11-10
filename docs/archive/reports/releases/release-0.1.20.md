# Ait 0.1.20 发布说明与验证

## 发布范围与来源

上一正式版 0.1.19 的 BUILD-INFO 与版本标签均指向 `a8db46db3874002e34741ba1bd66426d1382f0f0`。
本次 main 基线 `64b2c36bbbb2f0d3693583fffc4484ffc91785ce`，纳入 #490、#497、#494、#500、#496、#493、#486、#499、#501。
发布 Linux/macOS/Windows 桌面、两个独立 daemon 及当前工作流附带的网页客户端镜像。GitHub 正式 Release 与公开下载验证完成后暂停，确认后继续 COS、官网和服务端下载入口。
本次不触发 iOS/Android 构建或商店提交；iOS 工程版本仅作为发布源码元数据同步。

## 更新内容

- Codex 保留原生 sandbox、配置、审批及事件顺序，修复归档恢复、命令展开、子会话、终端状态和回退兼容性。
- OpenCode 停止请求使用独立取消信号；OMP 核对原生空闲状态，修复运行状态滞留。后台任务输出进入工具条目，同一轮 Thinking 合并显示。
- Antigravity 修复空工具输出导致的历史刷新失败，并兼容原生工具、子会话和中断恢复。
- 恢复 Provider 模型发现快照，清理侧边栏残留高亮。
- 精简 workspace.list 中 PR 检查详情，降低超出中继记录限制的风险；保留 DeepSeek Host 传输失败证据。
- iOS 后台通知注册和中心投递修复已进入源码，本次不发布移动二进制。

## 验证

15 个 Cargo 包、7 个 npm 包、锁文件、CHANGELOG 与 PKGBUILD 的 \_ait_version 同步至 0.1.20。第三方依赖和 Rust 实现没有因版本准备而改动。

- npm ci、verify:release、75 项发布工具测试、36 项原生/移动端发布元数据测试、桌面主进程构建和桌面/共享 UI 类型检查通过。
- 生产 Expo prebuild 与 CocoaPods 1.16.2 / Bundler 4.0.21 实际生成 iOS 0.1.20 / 1020999，严格 pod install --deployment 通过；仅保留版本字段与本地 Pod 版本/checksum，无关生成资源已恢复。
- [功能基线 CI](https://github.com/ait-app/ait/actions/runs/40503579156)已成功。最终合并提交 CI、三平台打包、签名公证与成品检查作为公开前门禁；结果记录在 Release Notes。
- 版本准备未修改 Rust 实现，本地 Rust 测试/覆盖率按 AGENTS.md 跳过。本次发布准备未进行真实模型、生产插件、移动设备或本机已安装 Electron 升级验收。

Windows 包未签名、不含 daemon，需要远程 Host。网页客户端镜像发布与生产部署分别记录。

## Test coverage

使用 Node v22.23.4 实测四个 JavaScript 版本/发布工具：**326/353 行（92.35%）**，分支 **109/114（95.61%）**，函数 **25/26（96.15%）**。

```sh
node --experimental-test-coverage --test-coverage-include=scripts/release-version.mjs --test-coverage-include=scripts/release-assets.mjs --test-coverage-include=scripts/release-layout.mjs --test-coverage-include=scripts/source-version.cjs --test scripts/source-version.test.cjs scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs scripts/release-layout.test.mjs scripts/release-workflow.test.mjs scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs scripts/edit-windows-executable.test.cjs
```

逐文件行覆盖率：release-assets.mjs 93.73%、release-layout.mjs 67.86%、release-version.mjs 100.00%、source-version.cjs 100.00%。与 0.1.19 的四文件范围相同，目标源码指纹一致：True。本次重新测量，Node patch 版本单独记录；不据此推断产品覆盖率或 Rust 变更覆盖。
精确命令、逐文件计数和源码指纹见 [JSON 证据](release-0.1.20-coverage.json)，最终发布提交需再次匹配这些指纹。
