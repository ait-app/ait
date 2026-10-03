# Ait 文档

本文档索引只覆盖当前 Ait：Rust daemon、Electron desktop、共享 Web/移动界面和仓库内 SDK。
源码布局与命名见 [ADR-072](decisions/daemon/adr-072-workspace-names-and-documentation.md)。

## 开始使用与开发

- [项目说明](../README.md)：开发入口、workspace 和验证命令。
- [桌面端](../apps/desktop/README.md)：Electron 启动、构建和打包。
- [移动端与 Web](../apps/mobile/README.md)：Expo、浏览器连接与原生构建。
- [daemon 使用与连接协议](operations/daemon.md)：配置、鉴权、数据目录和生命周期。
- [当前架构](architecture/README.md)：能力归属、依赖边界和数据所有权。

## 架构决策

- [ADR-076：Android 账户与中继客户端](decisions/clients/adr-076-android-account-relay.md)：共享账户会话、Android 安全存储、原生认证连接与下载。
- [ADR-075：Relay 协议定义与连接执行分离](decisions/clients/adr-075-relay-protocol-modules.md)：中继类型化消息、WebSocket 收发边界与单连接协商标识。
- [ADR-074：账户发现与按需反向中继](decisions/clients/adr-074-account-host-relay.md)：邮箱密码登录、主机注册、独立的控制与数据 WebSocket，以及单连接调度；[初版验证与覆盖率报告](reports/clients/account-host-relay-validation.md)。

[ADR 分类索引](decisions/README.md)按 daemon、工作区、Provider、客户端和品牌整理。
决策文档说明具体行为及其修订关系；当前目录与依赖图以当前架构和 ADR-072 为准。

- [桌面主窗口导航边界](decisions/clients/adr-073-desktop-renderer-navigation.md)：应用 preload 的来源限制。

## 运维与发布

- [发布指南](operations/releasing.md)：桌面 Release、Android Internal Testing、iOS TestFlight。
- [Ait 0.0.14 发布说明](reports/releases/release-0.0.14.md)：Diff 语法高亮、侧边栏统计与 Codex 推理等级。
- [Apple 本机构建](operations/apple-builds.md)：DMG、模拟器和 IPA。
- [Claude Code](operations/claude-code.md)：认证、原生会话与审批。
- [DeepSeek Harness](operations/deepseek-harness.md)：ACP 运行与模型配置。
- [语音与听写](operations/speech.md)：离线模型、后端配置和限制。

## 工程规范与验证

- [Rust style guide](policy/rust.md)：Rust 代码、测试、lint 与覆盖率规范。
- [文档规范](policy/documentation.md)：分类、维护和历史资料清理规则。
- [Provider 能力清单](plans/provider-parity.md)：当前能力与后续工作。
- [大 Diff 加载与超限降级](reports/workspace/large-diff-loading.md)：输出预算、连接保持与验证结果。
- [验证报告分类索引](reports/README.md)：实现、兼容性、发布及覆盖率记录。
- [2026-10-03 仓库审计](reports/daemon/repository-audit-2026-10-03.md)：审计范围、已修复问题与验证限制。
- [移动端 E2E](../apps/mobile/e2e/README.md)、[Maestro](../apps/mobile/maestro/README.md)：真实 daemon 连接验证。
- [品牌资产](../assets/brand/README.md)、[Paseo 来源](../paseo/README.md)：资产与许可证。

已移除实现的 CLI 流程、旧 daemon/worker 架构、旧 SQLite Project 设计和实验文档已清理。
相关历史保存在 Git 中；保留报告中的测试结果仅对应各报告注明的提交与测量范围。
