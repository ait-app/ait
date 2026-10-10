# Ait

<img src="logo.svg" alt="Ait logo" width="96" height="96" />

**在一个界面里管理多个编程 Agent。**

Ait 支持 Codex、Claude Code、OpenCode、DeepSeek Harness 等编程 Agent。
你可以在电脑上同时使用多个 Agent，查看文件和 Git 变更、操作终端，也可以从手机或另一台电脑连接，查看进度、发送消息和处理审批。

[下载安装](https://github.com/ait-app/ait/releases) · [更新记录](CHANGELOG.md)

## 可以做什么

- **Agent 对话**：发送任务，查看回复、工具调用和运行状态，处理权限审批。
- **项目与文件**：添加项目、创建 Git worktree，浏览文件、查看代码变更、使用终端。
- **远程连接**：从另一台电脑、Android 或 iOS 设备连接运行 Agent 的电脑。浏览器也可以直接连接后台服务。
- **Agent 设置**：选择 Agent 提供的模型和权限选项，继续已有会话。可用选项取决于安装的 Agent 及其版本。

Agent 在你连接的电脑上运行。使用本机 Agent 时无需登录 Ait 账户；
模型和账号使用各 Agent 自己的配置。

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
