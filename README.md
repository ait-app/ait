# Ait

<img src="logo.svg" alt="Ait logo" width="96" height="96" />

**本地优先的 AI 编程 Agent 工作台。**

Ait 把编程 Agent、项目工作区、Git 变更、终端和文件放在同一个界面中。
你可以在电脑上组织多个 Agent 的工作，也可以从另一台电脑或手机连接工作主机，继续查看进度、发送任务和处理审批。

[下载安装](https://github.com/ait-app/ait/releases) · [更新记录](CHANGELOG.md)

## 核心能力

- **统一管理 Agent**：接入 Codex、Claude Code、OpenCode、DeepSeek Harness 等本地运行时，查看对话、工具调用与执行状态。
- **围绕工作区开发**：管理项目与 Git worktree，在工作区内使用 Agent、浏览文件、查看 Diff 和操作终端。
- **跨设备继续工作**：桌面端使用本机服务；桌面、Android 和 iOS 可通过在线服务连接工作主机，浏览器支持直接连接 daemon。
- **沿用原生能力**：复用各 Agent 的认证、模型、会话与权限机制。具体功能取决于所选运行时及其版本，以应用中发现的能力为准。

Agent 在工作主机上执行，Ait 的 Rust 后台服务（daemon）负责会话与工作区管理；桌面、Web 和移动端共用界面与连接协议。
本地使用无需登录 Ait 在线服务；模型访问仍使用相应 Agent 的配置与认证。

## 开始使用

### 在电脑上使用

1. 从 [GitHub Releases](https://github.com/ait-app/ait/releases) 下载适合系统的桌面安装包：macOS Apple Silicon 或 Linux x86_64。
2. 在工作电脑上安装并完成所需编程 Agent 的认证，确保其可独立运行。
3. 启动 Ait，添加项目、进入工作区，选择可用的 Agent 开始任务。

桌面安装包内置 daemon，由应用自动启动。

### 从其他设备连接

在工作电脑的桌面应用中登录 **Online Service（在线服务）**，再在另一台电脑或手机上登录同一账户，选择在线主机。
桌面登录后默认同步内置 daemon；如果之前手动停止了同步，可在主机连接设置中重新启用。远程访问时，工作电脑及其服务需要保持运行。

Android 和 iOS 作为客户端连接电脑，不在手机上运行 daemon。

## 本地开发

准备 Git、Node.js 24、npm，以及 [rust-toolchain.toml](rust-toolchain.toml) 指定的 Rust 工具链。
仓库提供可选的 Nix 开发环境，运行 `nix develop` 可进入包含 Rust、Node.js 和构建工具的环境。

在仓库根目录运行：

```bash
npm ci
npm run dev:desktop
```

开发入口会构建共享包、Rust daemon 和 Electron 主进程，再启动界面与桌面应用。

需要单独运行后台服务时：

```bash
export AIT_SERVER_TOKEN="$(openssl rand -hex 32)"
cargo run -p daemon --bin daemon -- --listen 127.0.0.1:7316
```

客户端连接时填写主机地址、端口 `7316` 和上述 `AIT_SERVER_TOKEN` 的值。
`127.0.0.1` 仅允许本机连接。

移动端与 Web 的开发方式见 [客户端开发指南](apps/mobile/README.md)；
更多配置、架构和发布资料见 [文档索引](docs/README.md)。

## 许可证与致谢

Ait 采用 [Apache License 2.0](LICENSE)。客户端最初基于 [Paseo](paseo/README.md)，并在此基础上持续开发；
来源、修改范围与版权声明见 [Paseo 来源说明](paseo/README.md)及 [NOTICE](third-party/paseo/NOTICE)。
第三方代码保留各自的许可证与版权声明。
