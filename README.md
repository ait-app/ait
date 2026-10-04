# Ait

<img src="logo.svg" alt="Ait logo" width="96" height="96" />

Ait 是一个本地优先的多 Agent 管理器，统一在线协作平台、本地 Agent 运行时和任务界面。
本机服务使用 Rust，Electron 桌面与 Expo 界面共用当前 daemon、客户端 SDK 和连接协议。

## 开始开发

可选使用 Nix/direnv 进入包含 Rust、Clippy、LLVM coverage、Node.js 和构建工具的开发环境：

```bash
nix develop
# 或使用已审阅的 .envrc
direnv allow
```

```bash
npm ci
npm run dev:desktop
```

桌面入口会构建共享包、daemon 和 Electron 主进程，启动 Expo 与桌面应用。
移动端与 Web 的启动方式见 [apps/mobile](apps/mobile/README.md)。

独立运行 daemon：

```bash
export AIT_SERVER_TOKEN="$(openssl rand -hex 32)"
cargo run -p daemon --bin daemon -- --listen 127.0.0.1:7316
```

配置、认证、数据目录和协议见 [daemon 手册](docs/operations/daemon.md)。
已有 `AIT_SERVER_*` 配置保持兼容，桌面安装包携带 `resources/bin/daemon`。

## 通过账户连接其他主机

桌面和 Android 应用的默认账户服务地址为 `https://dash.ait-app.com:8443/api`。
打开 **Settings → Host → Account and online hosts**（或 **Add Host**），使用邮箱和密码登录。
首次启动时也可直接点击欢迎页的 **Account / Relay（账号登录 / Relay）**。
在电脑的桌面应用中登录同一账户后，该电脑即可上线。主机列表每 10 秒刷新一次，不显示当前机器。
选择一台主机，即可通过中继访问它的工作区、Agent、终端和文件。

登录凭据使用操作系统的安全存储保存，有效期内可自动恢复登录。
如果安全存储不可用，重启应用后需要重新登录。
使用自建服务时，在 **Service settings** 中填写 API 基础地址；留空则使用默认服务。
已保存的账户会继续使用原先配置的服务地址。`/hosts` 是管理页面，不是 API 基础地址。

Android 作为客户端连接在线电脑，不发布手机为工作主机。退到后台时暂停连接，回到前台后恢复。
浏览器和 iOS 尚未提供账户登录；Android 接入边界见
[ADR-076](docs/decisions/clients/adr-076-android-account-relay.md)。

## Workspace

| 目录           | 职责                                                                                                      |
| -------------- | --------------------------------------------------------------------------------------------------------- |
| `bins/daemon`  | Rust 服务入口、配置和组装                                                                                 |
| `crates/`      | `domain`、`model`、`protocol`、`api` 及 metadata/filesystem/provider/terminal/voice/schedule/browser 能力 |
| `apps/desktop` | `@ait/desktop` Electron 桌面和 daemon 生命周期                                                            |
| `apps/mobile`  | `@ait/mobile` 桌面、Web 与移动端共享界面                                                                  |
| `packages/`    | 本地私有 SDK、协议、高亮和音频模块                                                                        |
| `docs/`        | 当前架构、分类 ADR、运维、工程规范和验证报告                                                              |

本地包使用显式 `file:` 依赖，运行 `npm run verify:local-packages` 校验。
Rust 依赖方向见 [当前架构](docs/architecture/README.md)，所有修改遵循 [AGENTS.md](AGENTS.md)。

## OpenCode

使用本机已登录的 `opencode`，可用 `AIT_SERVER_OPENCODE_BIN` 指定可执行文件。
提供 Build 模式、模型发现、文本对话、单次审批、取消和恢复；
协议与能力限制见 [OpenCode 适配决策](docs/decisions/providers/adr-074-opencode-native-provider.md)。

## 验证与发布

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
npm run verify:local-packages
npm run verify:release
npm run check:docs
npm run test:release
npm run test:mobile-release
npm run build:desktop-main
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
```

迭代时只运行改动代码及直接相关行为的测试；Rust 提交准备运行完整 workspace 测试与覆盖率，
详见 [Rust 规范](docs/policy/rust.md)。
GitHub Release 支持 Linux x86_64 和 Apple Silicon；构建、签名与移动发布见
[发布指南](docs/operations/releasing.md)。更多资料见 [文档索引](docs/README.md)。

## 许可证

Ait 以 [Apache License 2.0](LICENSE) 开源，与 Paseo 一致。第三方代码保留其原有许可证和版权声明，
Paseo 来源与许可证见 [paseo](paseo/README.md)。
