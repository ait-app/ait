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

桌面、Android 和 iOS 应用的默认在线服务地址为 `https://dash.ait-app.com:8443/api`。
在欢迎页的 **Online Service（在线服务）**、**Settings → App → Online Service** 或
**Add Host → Online Service** 使用浏览器统一登录（邮箱、Google 或微信）。
桌面登录后默认同步内置 daemon；选择在线主机即可通过中继访问它的工作区、Agent、终端和文件。
浏览器仅使用直接连接。

登录入口与中继细节见 [移动端与 Web](apps/mobile/README.md)、
[ADR-083](docs/decisions/clients/adr-083-online-service-host-sync.md)、
[ADR-084](docs/decisions/clients/adr-084-ios-account-relay.md) 与
[ADR-123](docs/decisions/clients/adr-123-browser-only-account-login.md)。

## Workspace

| 目录           | 职责                                                                                                                  |
| -------------- | --------------------------------------------------------------------------------------------------------------------- |
| `bins/daemon`  | Rust 服务入口、配置和组装                                                                                             |
| `crates/`      | `domain`、`model`、`persistence`、`api`、`relay` 及 metadata/filesystem/provider/terminal/voice/schedule/browser 能力 |
| `apps/desktop` | `@ait/desktop` Electron 桌面和 daemon 生命周期                                                                        |
| `apps/mobile`  | `@ait/mobile` 桌面、Web 与移动端共享界面                                                                              |
| `packages/`    | 本地私有 SDK、协议、高亮和音频模块                                                                                    |
| `docs/`        | 当前架构、分类 ADR、运维、工程规范和验证报告                                                                          |

本地包使用显式 `file:` 依赖，运行 `npm run verify:local-packages` 校验。
Rust 依赖方向见 [当前架构](docs/architecture/README.md)，所有修改遵循 [AGENTS.md](AGENTS.md)。

## OpenCode

使用本机已登录的 OpenCode 1.x / 2.x，可用 `AIT_SERVER_OPENCODE_BIN` 指定可执行文件。
原生表单问答已验证于 2.0.26；缺少删除会话能力时，Ait 不创建查询或摘要会话。
通过官方 `opencode acp` 提供原生模式、模型发现、对话、审批、结构化问答、取消和恢复。
协议与能力限制见 [OpenCode ACP 适配决策](docs/decisions/providers/adr-115-opencode-acp-provider.md)。

## 验证与发布

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
npm run verify:local-packages
npm run verify:release
npm run check:docs
npm run test:release
npm run test:mobile-release
npm run build:desktop-main
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
npm test --workspace=@ait/desktop
npm run test:sdk
```

CI 另外运行 `@ait/protocol`、`@ait/highlight` 及选定的 `@ait/mobile` 测试，
完整列表见 [CI 配置](.github/workflows/ci.yml)。
本地只运行改动代码及直接相关行为的测试，提交与 PR 准备也不例外；完整 Rust 测试
（`cargo nextest run --workspace` 与 `cargo test --workspace --doc`）和覆盖率仅在明确要求时运行，
详见 [Rust 规范](docs/policy/rust.md)。
GitHub Release 支持 Linux x86_64 和 Apple Silicon；构建、签名与移动发布见
[发布指南](docs/operations/releasing.md)。更多资料见 [文档索引](docs/README.md)。

## 许可证

Ait 以 [Apache License 2.0](LICENSE) 开源，与 Paseo 一致。第三方代码保留其原有许可证和版权声明，
Paseo 来源与许可证见 [paseo](paseo/README.md)。
