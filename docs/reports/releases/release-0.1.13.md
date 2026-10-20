# Ait 0.1.13 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.12 实际来源为 `39675e3b7da3eee568843858fa87de7e65d5d8d2`，本次 main 基线为
`e3f0dac7131ac2e0f3786337031401e91c139555`，纳入 #407、#408、#409、#410、#413、#414、#415。
本次范围为桌面、两个独立 daemon、COS、官网及服务端下载入口。未授权移动发布，
不构建或提交 iOS/Android 安装包；按用户要求，GitHub 正式 Release 完成后暂停，再经确认继续镜像及入口。

## 更新内容

- 从桌面窗口拖拽区域排除弹窗、菜单和浮层，恢复控件点击。
- 将中继消息突发流量排队，按渲染进程确认窗口有序交付，避免积压导致断连。
- 支持 Pi 原生会话发现、导入、保留历史与继续对话；导入前应停止外部 Pi 写入。
- 对齐平台支持的工作区分屏行为，共享原生终端输入，并保持底部面板操作可见。
- 修正原生 diff 状态文本与 iOS 构建配置；共享源码变更不表示移动商店包已更新。
- 桌面 release 的 Android APK 构建改为显式开启 `build_android`；稳定标签推送默认跳过 Android。

## 发布准备与验证

测量提交 `effd79553a97fd3c2b173a91191a5b54767e88c9`。15 个 Cargo 包、7 个 npm 包、锁文件、PKGBUILD 与 CHANGELOG
同步为 0.1.13；本次准备未改动第三方依赖或 Rust 实现。

- `npm ci`、`npm run verify:release -- v0.1.13` 通过。
- 发布工具 61 项、移动发布契约 41 项全部通过；工作流条件实际执行六种事件/选项组合，
  验证稳定标签与默认手动入口跳过 Android、显式请求稳定版才运行，beta 仍跳过。
- `npm exec --workspace=@ait/desktop -- vitest run src/daemon/account-transport.test.ts`：7 项通过，
  覆盖 16/17/32/256 帧突发、同步/异步确认、窗口推进和停滞关闭。
- `npm run build:desktop-main`、桌面/共享界面类型检查、改动格式和定向 lint 通过。
- 本地文档检查通过 413 份 Markdown；添加本报告后重新核对链接。
- 按 AGENTS.md，版本准备没有 bins/crates Rust 改动，跳过本地 Rust 测试与覆盖率；
  不把上游报告中的历史测试计数或覆盖率当成本次实测。

正式公开仍需准备 PR 与 main 的适用 CI、Linux/macOS 构建、签名公证与成品门禁通过，
并验证全部附件、SHA256SUMS、GitHub digest、更新 SHA-512 与 BUILD-INFO。
上线和暂停状态记录在跨仓库交付报告。

## Test coverage

本次实际测量三个 JavaScript 发布工具：**265/292 行，90.75%**，分支 **75/80，93.75%**，
函数 **18/19，94.74%**。范围为 `release-assets.mjs`、`release-layout.mjs`、`release-version.mjs`；
不代表 Rust、TypeScript、移动原生代码或整个产品覆盖率。

```sh
node --experimental-test-coverage \
  --test-coverage-include=scripts/release-version.mjs \
  --test-coverage-include=scripts/release-assets.mjs \
  --test-coverage-include=scripts/release-layout.mjs \
  --test scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs \
  scripts/release-layout.test.mjs scripts/release-workflow.test.mjs \
  scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs
```

各文件行覆盖率分别为 92.91%、67.86%、100%。未覆盖资源脚本 37、123–129、209–218 行，
布局脚本 20–28 行，包含部分 CLI 入口和失败路径；无同范围可比基线，不推算历史变化。
[JSON 证据](release-0.1.13-coverage.json)记录精确范围、计数与源码指纹。
最终合并后仅报告变化时，用这些指纹核对源码等价。
