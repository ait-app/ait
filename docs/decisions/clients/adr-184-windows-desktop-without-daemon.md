# ADR-184：Windows Desktop 不附带内置 daemon

- 状态：Accepted
- 日期：2026-10-23
- 关系：补充 [ADR-048](adr-048-paseo-desktop-rust-launcher.md) 的平台边界。

## 决策

Windows Desktop 是连接远端主机的 Electron 客户端。构建共用 renderer 和主进程，
输出 x64 NSIS 安装包及 ZIP；不构建、暂存或打包 Rust daemon。macOS 和 Linux 的
daemon 资源改为平台专属资源，Windows 打包校验拒绝出现 `resources/bin`。
在非 Windows 主机上仅交叉构建未签名 ZIP，以 JavaScript 编辑 EXE 图标和版本资源，
避免 Wine 和 Intel NSIS 工具的主机架构限制。Windows 原生构建保留安装包和签名流程。

Windows 不自动启动 daemon、不探测默认 localhost，也不展示内置 daemon 的管理入口。
主进程拒绝内置 daemon 启动和重启，设置读取、更新及旧设置迁移均返回关闭状态。
账号登录、Hub 连接、远程 TCP/SSH transport 和其他桌面功能仍由 Electron 提供。

macOS/Linux 的内置 daemon 生命周期保持原有行为。Rust 服务及其领域边界不变。
`Release Ait` 调用独立 Windows 工作流，在 `windows-2025` runner 上构建安装包与 ZIP，
实际启动打包后的客户端验证欢迎页、账号桥接、无默认主机及内置 daemon 禁用状态。
Release 发布等待 Windows 与原有平台全部成功，收集安装包、ZIP、更新 YAML 和 blockmap，
统一写入 `SHA256SUMS`。`BUILD-INFO-windows.json` 记录该平台的源码和构建来源，便于
向旧版本追加 Windows 产物时保留原有平台的来源记录。

相关 PR 自动运行同一 Windows 工作流并保留构建产物，但不发布 Release。
main nightly 仍只构建 Linux/macOS。`npm run build:windows` 仅生成本地构建产物。
