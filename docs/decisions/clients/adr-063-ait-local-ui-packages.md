# ADR-063：Ait 本地 UI 包与原生测试连接

- 状态：Accepted
- 日期：2026-09-28
- 延续：ADR-061、ADR-062

## 决策

1. 当前 npm workspace 只包含 `@ait/mobile`、`@ait/desktop`、`@ait/client`、`@ait/protocol`、`@ait/highlight`、`@ait/expo-two-way-audio`，均为 private。四个共享库的源码保留在本仓库 `packages/`，每条内部依赖使用明确的 `file:` 路径；不从 Paseo npm 包或仓库外源码加载实现。
2. App、桌面端、包间 import、构建/测试/发布脚本与 CI 一起使用 `@ait/*`。统一使用根 `package-lock.json`；移除音频库遗留的独立包锁。`verify:local-packages` 验证 workspace 身份、本地依赖、锁文件链接及退役依赖缺失。
3. 删除已退役的 `packages/relay`、`packages/plugin`、只服务插件的 npm registry fixture、Vitest relay alias 及 Maestro relay 流程。保留上游许可证、来源说明，以及兼容客户端仍使用的 wire 类型；包名迁移不等于 Ait Rust 协议重写。
4. 原生音频继续使用既有 `ExpoTwoWayAudio` 原生模块标识，Swift/Kotlin 实现来自同一仓库。npm namespace 改名不改变原生 bridge；上游 MIT 许可与署名保留。
5. Maestro 必须显式提供 `AIT_MAESTRO_SERVER_URL` 和 `AIT_MAESTRO_TOKEN`，先通过 Bearer 验证 `/v1/server/info`，再用 App 的生产 Rust adapter 连接 `/v1/ws`。不使用旧 `/ws`、`/api/health`、relay 产物或隐式开发服务器端口。
6. Maestro 三个连接 harness 共用认证、项目准备与清理 helper，Android 根据实际端口执行 ADB reverse；原生 UI 使用高级连接 URI 填充地址、TLS 和 token。YAML 通过解析后替换变量安全渲染，所有嵌套 flow 使用一致 app id，退出清理含 test token 的渲染文件。

## 兼容与范围

`apps/desktop` 是现有桌面源码目录，本次不移动目录；当时保留的 `dev:paseo` / `build:paseo` 作为旧命令别名转发到 `dev:desktop` / `build:desktop-main`，之后已删除。客户端高层兼容符号、IPC 和持久化键不随包名机械替换。真实 Rust API 仍是服务端边界；App transport 负责映射兼容消息。

## Test coverage

不适用：本次未修改 `bins/` 或 `crates/` 中的 Rust 行为，按仓库约定不运行 Rust 测试或覆盖率。TypeScript 行覆盖率未测量；构建、类型检查、本地依赖校验、Maestro helper 测试和真实 Ait E2E 结果见[验证报告](../../reports/clients/ait-local-ui-packages.md)。
