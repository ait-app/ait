# Linux 系统服务与桌面连接：修复验证

日期：2026-10-14。基于 main `96a43240`（0.1.2）加本 PR 修改；实机部署在 `13ce800ccb31d60a492338047a3c2f83d1c694c9`（0.1.1）上应用相同修复。相关生产文件可以原样移植到当前 main。本报告分别记录实机行为和 PR 基线检查，不将部署验收当成当前提交的全量测试。

## 修复行为

系统服务以 `--service` 原子发布私有 `service-connection.json`。桌面明确 external 模式，或在 auto 模式发现该记录时，仅认证连接已有服务；缺少记录、认证失败、连接超时和重启失败不授权 spawn。记录不被客户端消费，系统服务 PID 不转化为桌面子进程。

readiness 只使用认证握手，daemon status 读取当前 Provider snapshot。Provider discovery 独立运行，慢模型目录不阻塞基础连接。服务模式的停止操作断开桌面，Restart 通过认证 RPC 重建 runtime 并确认新的 instance ID，然后更新桌面连接。

契约见 [ADR-148](../../decisions/clients/adr-148-service-owned-desktop-daemon.md)，安装步骤见 [daemon 运维](../../operations/daemon.md)。

## CachyOS / KDE 实机验收

系统使用 KDE Wayland；systemd 用户服务启用 linger。服务运行 PATH 包含用户的 `.local/bin` 与 `.cargo/bin`，正确识别 Codex、Claude 和 OpenCode。原 unit 伪造桌面留存记录的启动/重载脚本已删除；原生记录为 UID 1000 所有、0600 权限。

| 场景                 | 实际结果                                                                                                             |
| -------------------- | -------------------------------------------------------------------------------------------------------------------- |
| 真正重启且不登录 KDE | boot ID 已变化；只有 greeter 与 linger manager，没有 KDE 用户会话或 plasmashell，桌面 unit inactive，daemon 自动运行 |
| 无人登录的远程连接   | Mac 经 SSH transport 完成认证 WebSocket；身份与原生记录匹配                                                          |
| 冷基础状态           | Linux 22 ms，Mac 10 ms；冷 OpenCode 目录约 14 s，不拖住基础状态                                                      |
| 模型目录             | Codex 7、OpenCode 14、Claude 5，分别调用实际目录接口                                                                 |
| 真实终端             | Linux 与 Mac 新建 PTY，执行命令并捕获独立输出行 `AIT_NATIVE_SERVICE_PTY_OK`                                          |
| 真实 Agent           | Mac 客户端创建 Codex Agent，timeline 中 assistant_message 的实际文本为 `AIT_NATIVE_SERVICE_AGENT_OK`                 |
| 登录并打开桌面       | 连接已有系统服务，managementMode=external、ownedByDesktop=false，daemon PID 不变                                     |
| 桌面终端             | UI 输入命令，真实 PTY 输出独立行 `AIT_POST_LOGIN_UI_PTY_OK`                                                          |
| 关闭与重开桌面       | 同一持续 PTY 分别执行新的命令；daemon PID 不变                                                                       |
| 强杀与重新打开桌面   | 只 SIGKILL 桌面主进程；同一持续 PTY 继续执行命令，daemon PID 不变                                                    |
| UI Restart           | 点击实际按钮并确认原生弹窗；PID 不变、instance ID 变化、systemd NRestarts=0，UI 自动回到 localhost 在线状态          |
| UI Restart 后功能    | 重新获取三类目录；真实 UI Codex 回复 `AIT_POST_LOGIN_UI_AGENT_OK`；新建 UI 终端输出 `AIT_POST_RESTART_UI_PTY_OK`     |

用户登录前后的验收均已执行。最终 UI Restart 后系统 daemon PID 为 766，端口 7316 唯一监听。临时 Electron 调试端口、runtime drop-in、SSH 隧道和 xdotool 均已清理，桌面恢复正常启动；清理期间持续 PTY 仍能执行命令，随后回收测试 PTY。

## 范围与独立问题

- Restart 重建 runtime，保留 systemd 的进程所有权；更新二进制和环境仍应由 systemd 重启进程。runtime 重启会回收旧 PTY，本次分别验证桌面退出期间的会话连续性和 daemon 重启后新建 PTY 的可用性。
- Mac 使用真实远程客户端，未执行 Mac 桌面 UI 验收。Linux 桌面实际目录、Codex 对话和终端均执行了验收。
- Claude/OpenCode 本轮验证 discovery 与模型目录，未执行它们的真实对话。
- 本机没有独立 daemon 中心授权，服务未启用 `run --headless`。无人登录的云端 Relay 未通过验收；桌面还显示过独立的 active login / lease 冲突。这些问题不归因于桌面 spawn 冲突，本次不改变 HostLink 授权机制。
- OpenCode 冷目录仍约 14–18 s；没有通过增加接管等待时间掩盖它。

## PR 基线检查

执行环境：macOS aarch64，Nix 的 Rust 1.98.1，默认 features；源码为 main `96a43240` 加本 PR 修复。

定向 Rust 检查包括私有记录发布/替换、真实 service binding、慢 Provider discovery 时 status/config 可用、API connection dispatch。桌面测试覆盖认证失败与超时保留记录、不 spawn、显式 external 缺少记录、文件权限/符号链接、服务重启身份变化及重新打开连接。移动端测试覆盖现有主机连接更新及系统服务的桌面 restart 路径。

以下检查均通过，未运行完整 Rust workspace 测试。Rust 命令通过 Nix 使用仓库固定工具链，并复用本机 CARGO_TARGET_DIR 编译缓存。

| 命令                                                                                                | 结果            |
| --------------------------------------------------------------------------------------------------- | --------------- |
| `cargo test -p daemon --bin daemon service_`                                                        | 3 通过          |
| `cargo test -p daemon --test process unix::daemon::`                                                | 4 通过          |
| `cargo test -p api connection::dispatch::tests`                                                     | 13 通过         |
| `cargo clippy --locked --workspace --all-targets -- -D warnings`                                    | 通过，无警告    |
| `cargo build --workspace`                                                                           | 通过            |
| `cargo fmt --all --check`                                                                           | 通过            |
| Desktop 定向 Vitest：service-connection / rust-daemon / desktop-settings，设置实际 `AIT_SERVER_BIN` | 31 通过，无跳过 |
| Mobile 定向 Vitest：daemon-start-service / daemon-lifecycle                                         | 36 通过         |
| `npm run typecheck --workspace=@ait/desktop` / `@ait/mobile`                                        | 均通过          |
| 修改文件 Oxlint / Oxfmt                                                                             | 均通过          |
| `npm run check:docs` / `git diff --check`                                                           | 均通过          |

默认 Desktop 测试曾跳过 6 个真实子进程用例；随后使用本次实际构建的 daemon 重新执行上述三个文件，所有用例均执行。npm 依赖按 lockfile 安装并应用仓库 postinstall patches。Windows 原生行为未执行。

## Test coverage

**not measured**。本次只请求提交 PR，没有请求覆盖率；遵循当前仓库 policy 使用定向测试。测试通过数量不代表行覆盖率，没有本次覆盖率 artifact 或可比基线。如后续请求测量，将对受影响的 daemon/API 和相关行为使用 cargo llvm-cov，并记录命令、revision、covered/total lines 与可审阅 artifact。
