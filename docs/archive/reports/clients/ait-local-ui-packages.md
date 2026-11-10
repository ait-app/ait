# Ait 本地 UI 包迁移与 Maestro 修复

日期：2026-09-28。开发阶段代码为 `22ade32`（基于 `b04726a`）；提交准备同步 `main` 的 `2f4f6a9`，保留聊天搜索更新及发布文档，只手工调整文档目录和测试中的包名导入。

## 结果

- 六个私有 workspace 使用 `@ait/mobile`、`@ait/desktop`、`@ait/client`、`@ait/protocol`、`@ait/highlight`、`@ait/expo-two-way-audio`。四个共享库源码均在本仓库 `packages/`；内部依赖明确使用 `file:`，无需从 Paseo npm 包取实现。
- App、桌面、包间 import、脚本、CI 和根锁文件同步更新。新增 `verify:local-packages` 校验私有身份、源码路径和锁文件链接；CI 执行同一检查及 Maestro helper 单测。
- 删除已退役 relay/plugin 包、相关构建入口和 Vitest alias、插件 registry fixture、两个 Maestro relay 流程；根安装图不再包含 `@getpaseo/*`。
- Maestro 使用 `apps/mobile/maestro` 正确路径，显式认证 Ait `/v1/server/info`、通过生产 Rust transport 连接 `/v1/ws`；Android 读取配置端口。共用项目创建/打开/清理和 YAML 渲染，原生 UI 接收 host、port、TLS、token，渲染文件权限为 0600 并在退出时删除。
- 音频库为 Ait 私有本地模块，iOS/Android 保留原生 `ExpoTwoWayAudio` 标识与第三方许可；删除该库遗留独立 lockfile，版本验证统一检查根锁文件。

## 开发阶段验证

- `npm install --package-lock-only --ignore-scripts --offline`、`npm ci --offline --ignore-scripts`、`npm run postinstall`：通过；全新依赖图以本地链接安装六个 Ait workspace。
- `npm run verify:local-packages`、`npm run verify:release`、`npm run test:release`：通过，12 项发布脚本测试通过。
- `npm run build:desktop-main`：通过，包含 protocol、client、highlight、audio 和 desktop 构建。
- `npm run typecheck` 分别针对以上六个 `@ait/*` workspace：通过；App 在新增 Maestro helper 后最终复查通过。
- `npm run test --workspace=@ait/mobile -- maestro/support`：2 文件 / 11 项通过，覆盖端口、TLS、token、拒绝旧连接、嵌套 YAML、变量引用和文件权限。
- 桌面打包、启动、daemon manager 与 dev runner：4 文件 / 15 项通过；最终 dev runner 4 项复查通过。
- SDK `daemon-client-transport`：10 项通过；App Rust adapter、host runtime、桌面 transport、appearance：6 文件 / 118 项通过。
- `E2E_AIT_SERVER_BIN="$PWD/target/debug/daemon" npm run test:e2e --workspace=@ait/mobile -- e2e/browser/ait-server.spec.ts e2e/browser/maestro-ait.spec.ts e2e/browser/workspace-model-restart.spec.ts e2e/browser/daemon-lifecycle.spec.ts`：7 项通过。
- Node/tsx CLI 的 Ait readiness 与 ADB 端口输出通过最终 Maestro E2E 复跑（1 项）；端口用 stdout 纯文本输出，避免彩色终端污染 shell 参数。
- `expo-modules-autolinking resolve --platform apple --project-root apps/mobile --json` 与 Android 对应命令：均发现本仓库的 `@ait/expo-two-way-audio@0.0.7`。
- 修改的 TS/JS 文件通过 `oxfmt --check` 与 `oxlint -A no-empty-pattern`；Maestro shell 通过 `bash -n`；`git diff --check` 通过。

## 提交准备验证

以下检查在同步 `2f4f6a9` 后的 PR 代码上运行：

- `npm run verify:local-packages`、`npm run verify:release` 和 12 项发布脚本测试：通过。
- `npm run build:desktop-main` 与六个 `@ait/*` workspace 类型检查：通过。
- `npm run test --workspace=@ait/protocol`：66 文件 / 746 项通过。
- `npm test --workspace=@ait/desktop`：53 文件 / 373 项通过，13 项跳过。
- App 的 i18n、Rust adapter、Maestro、native release 和聊天搜索模型检查：8 文件 / 93 项通过。
- 469 个修改的 TS/JS 文件通过 `oxfmt --check` 与 `oxlint -A no-empty-pattern`，无警告。
- 重建最新 main 的 `daemon` 后执行上述四个 E2E 文件：7 项通过，包含实际 Node/tsx Maestro CLI 入口。
- 相对 `main` 没有 Rust 文件变化，按仓库规则跳过 Rust 测试；原生手势测试的环境限制仍适用。

## Test coverage

不适用：未修改 `bins/` 或 `crates/` 的 Rust 代码，按仓库约定跳过 Rust 测试和覆盖率。本轮 TypeScript 行覆盖率未测量，不使用此前 Rust 覆盖率代替。以上为定向测试执行结果，非覆盖率百分比。

## 限制

当前环境未安装 Maestro CLI，因此没有执行真机/模拟器上的原生手势流程，也未重新编译 iOS/Android 原生应用。原生自动链接和共享准备逻辑已分别验证；完整 native UI 仍按 [Maestro README](../../../../apps/mobile/maestro/README.md) 在装有 Maestro 的设备环境执行。

本次不改写继承的客户端高层 API 符号和兼容 wire 类型，不改变 IPC 或持久化键。原工作区此前的 iOS 改动保持原样，所有本轮修改位于独立工作树。
