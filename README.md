# Ait

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo-dark.svg" />
  <img src="assets/brand/logo.svg" alt="Ait logo" width="96" height="96" />
</picture>

**在一个界面里管理多个编程 Agent。**

Ait 支持 Codex、Claude Code、OpenCode、DeepSeek Harness、Pi、Oh My Pi 等编程 Agent。
你可以在电脑上同时使用多个 Agent，查看文件和 Git 变更、操作终端，也可以从手机或另一台电脑连接，查看进度、发送消息和处理审批。
Ait 提供中央中继服务：设备不在同一网络时，登录同一账户即可连接在线工作电脑，无需为工作电脑配置公网 IP 或路由器端口转发。

[下载安装](https://github.com/ait-app/ait/releases) · [更新记录](CHANGELOG.md)

## 可以做什么

- **Agent 对话**：发送任务，查看回复、工具调用和运行状态，处理权限审批。
- **项目与文件**：添加项目、创建 Git worktree，浏览文件、查看代码变更、使用终端。
- **中央中继与远程连接**：通过中央中继，从另一台电脑、Android 或 iOS 设备跨网络访问运行 Agent 的电脑，继续对话、查看文件和操作终端。浏览器也可以直接连接后台服务。
- **Agent 设置**：选择 Agent 提供的模型和权限选项，继续已有会话。可用选项取决于安装的 Agent 及其版本。

Agent 在你连接的电脑上运行。使用本机 Agent 时无需登录 Ait 账户；
模型和账号使用各 Agent 自己的配置。

## 开始使用

### 在电脑上使用

1. 从 [GitHub Releases](https://github.com/ait-app/ait/releases) 下载适合系统的桌面安装包：macOS Apple Silicon 或 Linux x86_64。
2. 在工作电脑上安装并完成所需编程 Agent 的认证，确保其可独立运行。
3. 启动 Ait，添加项目、进入工作区，选择可用的 Agent 开始任务。

桌面安装包内置 daemon，由应用自动启动。

Arch Linux 用户也可以从 AUR 安装：[`ait-bin`](https://aur.archlinux.org/packages/ait-bin) 为正式版预编译包，[`ait-git`](https://aur.archlinux.org/packages/ait-git) 在本机从 `main` 源码编译，两者任选其一（例如 `paru -S ait-bin`）。

### 从其他设备连接

1. 在工作电脑的桌面应用中打开 **Online Service（在线服务）**，通过浏览器统一登录（邮箱、Google 或微信）。入口位于欢迎页、**Settings → App → Online Service** 或 **Add Host → Online Service**。
2. 在另一台电脑、Android 或 iOS 应用中登录同一账户，选择在线主机，即可通过中央中继连接。

应用默认使用 `https://aitapp.cn/api` 作为在线服务地址。
两端只需能访问在线服务，无需处于同一局域网，也无需开放工作电脑的入站端口。中继负责转发连接，Agent 和项目文件仍在工作电脑上。
桌面登录后默认同步内置 daemon；如果之前手动停止了同步，可在主机连接设置中重新启用。远程访问时，工作电脑及其服务需要保持运行。

Android 和 iOS 作为客户端连接电脑，不在手机上运行 daemon。
也可以在 [网页客户端](https://aitapp.cn/client/) 登录同一账户，通过中央中继连接在线主机。
登录入口与连接细节见 [客户端开发指南](apps/mobile/README.md)。

### 在 Linux/macOS 上独立运行 daemon

没有桌面环境的服务器或需要长期在线的工作电脑，可以单独运行 daemon。先在这台机器上安装并认证需要使用的编程 Agent。

在仓库根目录使用 [rust-toolchain.toml](rust-toolchain.toml) 指定的 Rust 工具链及系统 C/C++ 构建工具编译：

```sh
cargo build --locked --release -p daemon --bin daemon
./target/release/daemon login
./target/release/daemon run --headless
```

首次登录会显示网页地址和验证码。在任意设备浏览器中完成登录并批准机器接入，再运行启动命令。
另一台设备登录同一 Ait 账户后，即可选择这台在线主机。机器名默认取系统 hostname，也可在首次登录时用 `login --name build-server` 指定；后续显示名称在网页 Host 管理中修改。

- **重启**：直接执行 `daemon run --headless`，自动恢复已保存的机器授权，无需每次登录。
- **本地令牌**：不需要手动设置 `AIT_SERVER_TOKEN`；headless 首次启动会自动生成私有令牌，后续复用。显式设置该环境变量时优先使用它。
- **数据目录**：默认为 `~/.ait-server`。使用同一系统用户运行登录与服务；自定义目录时，各命令均传入 `--data-dir /path/to/data`。
- **查看与退出**：`daemon status --json` 查看状态。需要重新授权或退出时，先停止服务，再执行 `daemon login` 或 `daemon logout`。
- **后台常驻**：`run --headless` 在前台运行；长期运行可交给 Linux systemd 或 macOS launchd 管理。无需开放入站端口，默认本地监听为 `127.0.0.1:7316`。

以上 `daemon` 指编译产物或已放入 PATH 的程序。Linux 默认构建依赖 glibc，复制到其他机器前需确认 CPU 架构及系统库兼容；macOS 需在对应平台编译。离线语音引擎包含在构建中，模型另行下载。
完整的授权恢复、一次性 token 接入和凭据说明见 [无人值守 daemon 操作指南](docs/operations/headless-daemon.md)。

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
参与开发时请遵循 [仓库规范](AGENTS.md)和 [Rust 规范](docs/policy/rust.md)；CI 检查列表见 [CI 配置](.github/workflows/ci.yml)。

## 许可证与致谢

Ait 采用 [Apache License 2.0](LICENSE)。客户端最初基于 [Paseo](paseo/README.md)，并在此基础上持续开发；
来源、修改范围与版权声明见 [Paseo 来源说明](paseo/README.md)及 [NOTICE](third-party/paseo/NOTICE)。
第三方代码保留各自的许可证与版权声明。
