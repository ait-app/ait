# Ait 0.1.21 发布说明与验证

## 发布范围与来源

上一正式版 0.1.20 的最终 BUILD-INFO 指向 `bca1f98b8338f2bd6dbdd7f4e733337a564ce466`；该版本重建保留了原标签，比较基线使用实际发布源码。
本次 main 功能基线 `89ea11dd3724c1b08d33508c15d4531a1176d52d`，纳入 #506、#508、#507、#509。
发布 Linux/macOS/Windows 桌面、两个独立 daemon 及现有 workflow 的网页客户端镜像。GitHub Draft 附件校验完成后暂停，收到继续指令后再公开 Release、同步 COS、官网和服务端下载入口。
本次不触发 iOS/Android 构建或商店提交；iOS 工程版本作为发布源码元数据同步。

## 更新内容

- 接入 MiniMax Code 原生 ACP。
- 在线服务设置增加账户删除及统一账号中心入口。
- Codex 子 Agent 可以向父会话回传消息。

## 验证

15 个 Cargo 包、7 个 npm 包、锁文件、CHANGELOG 和 PKGBUILD 的 \_ait_version 同步至 0.1.21；第三方依赖与 Rust 实现未因版本准备改变。

- npm ci、verify:release、75 项发布工具测试、36 项移动端发布元数据测试、桌面主进程构建和桌面/共享 UI 类型检查通过。
- 生产身份 Expo prebuild 实际生成 iOS 0.1.21 / 1021999；CocoaPods 1.16.2、Bundler 4.0.21 重新生成锁文件，pod install --deployment 通过。仅保留版本字段与本地 Pod 版本/checksum，无关生成资源已恢复。
- [功能基线 CI](https://github.com/ait-app/ait/actions/runs/40638632547)、最终合并提交 CI、三平台打包、签名公证与成品门禁的最终状态在 Release Notes 记录，成功后才交付 Draft。
- 版本准备未修改 Rust 实现，按 AGENTS.md 跳过本地 Rust 测试和覆盖率。本次未进行真实模型、生产插件、移动设备或本机已安装 Electron 升级验收。

Windows 包未签名、不含 daemon，需要远程 Host。网页客户端镜像发布与生产部署分别记录。

## Test coverage

使用 Node v22.23.4 实测四个 JavaScript 版本/发布工具：**326/353 行（92.35%）**，分支 **109/114（95.61%）**，函数 **25/26（96.15%）**。

```sh
node --experimental-test-coverage --test-coverage-include=scripts/release-version.mjs --test-coverage-include=scripts/release-assets.mjs --test-coverage-include=scripts/release-layout.mjs --test-coverage-include=scripts/source-version.cjs --test scripts/source-version.test.cjs scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs scripts/release-layout.test.mjs scripts/release-workflow.test.mjs scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs scripts/edit-windows-executable.test.cjs
```

逐文件行覆盖率：release-assets.mjs 93.73%、release-layout.mjs 67.86%、release-version.mjs 100.00%、source-version.cjs 100.00%。与 0.1.20 的四文件范围相同，目标源码指纹一致：True；本次重新测量，不据此推断产品或 Rust 变更覆盖率。
精确命令、逐文件计数和源码指纹见 [JSON 证据](release-0.1.21-coverage.json)，最终发布提交须再次匹配这些指纹。
