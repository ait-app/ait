# Ait 0.1.14 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.13 的 BUILD-INFO 实际来源为 `a0a7044352f51d2d7cdb8485baecd76fb4fbf7db`。
本次 main 基线为 `1c0fddc32ae58f4f6f2e3adf74395de8258f7c16`，纳入 #419、#420、#421、#422、#423、#425、#427、#429、#416、#430、#433。
本次发布桌面、独立 daemon，以及后续 COS、官网和服务端下载入口。
按用户要求，GitHub 正式 Release 完成后暂停，等待确认再继续镜像和入口；不执行 iOS 或 Android 构建与商店提交。

## 更新内容

- Codex 在 OOM 重启后恢复中断任务，诊断保留具体的上游失败原因。
- 减少 OpenCode 模型目录冷启动开销，启动前探测就绪状态，并恢复停滞会话。
- Pi 原生会话发现保留项目身份，导入与恢复时保留用户图片历史。
- 兼容 DeepSeek Harness 的空事件确认，保持完成事件交付一致。
- 增加 daemon 侧原生推送交付及本地通知设置；移动安装包仍需独立发布。
- Provider 行为测试迁入所属 crate，daemon 测试保留主机与跨能力集成范围。
- 更新 iOS Xcode Cloud/CocoaPods 配置和 Android Gradle 发布流程；桌面 Release 工作流已移除移动构建。

## 发布准备与验证

15 个 Cargo 包、7 个活跃 npm 包、锁文件、PKGBUILD 和 CHANGELOG 同步为 0.1.14。
准备工作只修改版本与文档，不修改第三方依赖或 Rust 实现。

- `npm ci`、`npm run verify:release -- v0.1.14` 通过。
- 发布工具测试 61 项、移动发布契约测试 45 项通过，均无失败或跳过。
- `npm run build:desktop-main`、桌面与共享 UI 类型检查、变更格式检查通过。
- 版本准备未修改 bins/crates Rust 实现，按 AGENTS.md 跳过本地 Rust 测试和覆盖率。
- 基线 [main CI](https://github.com/ait-app/ait/actions/runs/39584696127)通过；正式公开前仍需准备 PR、最终 main CI、两平台发布构建、macOS 签名公证及成品门禁全部通过。
- 发布时核对不可变标签、全部附件、SHA256SUMS、GitHub digest、更新 SHA-512 和 BUILD-INFO；具体上线与暂停状态记录在发布工作区交付报告。

本次未运行移动安装包、真机或真实外部 Provider 验收；不以脚本覆盖率代替这些验证。

## Test coverage

本次重新测量三个 JavaScript 发布工具，覆盖 **265/292 行（90.75%）**、
分支 **75/80（93.75%）**、函数 **18/19（94.74%）**。
测量基线 `1c0fddc32ae58f4f6f2e3adf74395de8258f7c16`；准备工作区包含版本修改，最终合并后以证据中的源码指纹核对等价关系。
范围为 `release-assets.mjs`、`release-layout.mjs` 和 `release-version.mjs`，不代表 Rust、TypeScript、移动原生或整个产品覆盖率。

```sh
node --experimental-test-coverage \
  --test-coverage-include=scripts/release-version.mjs \
  --test-coverage-include=scripts/release-assets.mjs \
  --test-coverage-include=scripts/release-layout.mjs \
  --test scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs \
  scripts/release-layout.test.mjs scripts/release-workflow.test.mjs \
  scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs
```

三个文件的行覆盖率依次为 92.91%、67.86%、100%。未覆盖资源脚本 37、123–129、209–218 行，
布局脚本 20–28 行，主要包含部分 CLI 入口和失败路径。
与 v0.1.13 相比，三个目标文件源码指纹一致，本次重新执行所得覆盖行数与比例均无变化。
[JSON 证据](release-0.1.14-coverage.json)记录精确范围、命令、计数及源码指纹。
