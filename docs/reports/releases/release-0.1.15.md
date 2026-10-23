# Ait 0.1.15 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.14 来源为 `29fa17f61454dc22477108cc6737187dc1dc74fd`。
本次 main 基线 `b8fadd89e7cf5ca64f609c215a69279b3bf56459`，纳入 #426、#439、#432、#428、#437、#443、#444、#438、#447。
发布桌面、独立 daemon，以及后续 COS、官网和服务端下载入口。
按用户要求，GitHub 正式 Release 完成后暂停，确认后继续；不执行 iOS 或 Android 构建与商店提交。

## 更新内容

- 新增 Oh My Pi 独立原生 RPC Provider，支持流式输出、审批、会话历史、恢复与子 Agent 事件。
- 接入可断线续传的账号 Hub、Host 事件发布与桌面账号服务；旧中心或目标没有 Hub 能力时保留 Relay 兼容路径。
- 上传文件名保留中文等 Unicode 字符。
- OpenCode 跟随 session_move 后的会话，Claude 原生历史读取上限提高至 128 MiB。
- 桌面模型选择器按 Provider 行数调整高度，修复 Web tooltip 定位和首条消息被悬浮标题遮挡。

## 发布准备与验证

15 个 Cargo 包、7 个活跃 npm 包、锁文件、PKGBUILD 和 CHANGELOG 同步为 0.1.15。
版本准备只修改版本和文档，不修改第三方依赖或 Rust 实现。

- `npm ci`、`npm run verify:release -- v0.1.15`、发布工具 61 项测试通过。
- `npm run build:desktop-main`、桌面与共享 UI 类型检查、变更格式与文档链接检查通过。
- 本地 Rust 测试与覆盖率依据 AGENTS.md 跳过；基线 [main CI](https://github.com/ait-app/ait/actions/runs/39770401894)通过。
- 正式公开前核对准备 PR、最终 main CI、两平台发布构建、macOS 签名公证、Linux 安装与生命周期成品门禁。
- 发布产物逐项核对附件、SHA256SUMS、GitHub digest、更新 SHA-512 与 BUILD-INFO 来源。

Hub 账号能力依赖中心部署对应接口；中心原生通知需配置提供方凭据并显式启用 `AIT_SERVER_HUB_NOTIFICATIONS=true`。
本次未验证生产 Hub/APNs/FCM 投递、真机移动安装包或真实外部 Provider；旧中心继续通过 Relay 兼容路径连接。

## Test coverage

本次重新测量三个 JavaScript 发布工具，覆盖 **265/292 行（90.75%）**、分支 **75/80（93.75%）**、函数 **18/19（94.74%）**。
测量基线 `b8fadd89e7cf5ca64f609c215a69279b3bf56459`，准备工作区包含版本与文档修改；最终合并提交通过源码指纹核对等价关系。
范围为 `release-assets.mjs`、`release-layout.mjs` 和 `release-version.mjs`，不代表 Rust、TypeScript 或整个产品覆盖率。

```sh
node --experimental-test-coverage \
  --test-coverage-include=scripts/release-version.mjs \
  --test-coverage-include=scripts/release-assets.mjs \
  --test-coverage-include=scripts/release-layout.mjs \
  --test scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs \
  scripts/release-layout.test.mjs scripts/release-workflow.test.mjs \
  scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs
```

三个文件行覆盖率依次为 92.91%、67.86%、100%。未覆盖资源脚本 37、123–129、209–218 行，布局脚本 20–28 行，主要为 CLI 和失败路径。
与 0.1.14 相比目标源码指纹一致，本次重新实测的覆盖行数与比例无变化。[JSON 证据](release-0.1.15-coverage.json)记录精确范围、命令和计数。
