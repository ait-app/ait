# daemon 与 desktop 运行验证

日期：2026-10-22。基线：`3602d270`；修复对应本报告所在提交（PR #447）。
环境：macOS、Electron 44.2.0、当前源码构建的 Rust daemon 与 Electron UI。
Hub 联调使用本机 `ait-server` 提交 `7128bce` 和隔离 PostgreSQL 17。

## 结论与修复

本地 daemon/desktop 基本流程通过。真实 Hub 联调发现 Hub-only Host 无法连接：
远端 serve 已在线，旧 Relay control 离线；账号主机列表仍将其标为离线，
共享连接资格又要求旧 Relay 的实例及模式字段，桌面 Hub open 同样要求旧实例。

修复后，现有账号主机列表合并同一中心的 Hub presence，连接资格可以来自 Hub。
Hub 会话按中心认证的 serve node 路由，并校验账号记录的 server ID、当前实例和协议；
旧 Relay 会话仍校验 visit 绑定实例。未增加产品页面或导航入口。
边界补充在 [ADR-182](../../decisions/clients/adr-182-hub-account-client.md)。

另外更新了设置 E2E：去掉已删除的 Integrations/Pair device 入口，纳入现有 Online Service，
按当前打包发布说明验证 HTTP 404 回退，并为首次异步内容加载保留 30 秒等待预算。

## 实际进程验证

所有场景使用独立临时 daemon 数据目录和 Electron profile。
模型响应使用仓库 Codex app-server Python fixture；没有调用真实模型服务。

| 场景                            | 结果                                                                                                                                                                                                                                                                           |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `rust-startup.e2e.mjs`          | Electron 自动启动 daemon、鉴权、终端订阅、创建首个项目/工作区/Agent、两轮消息和 steer、两次重启恢复历史、退出清理子进程均通过；无 renderer error                                                                                                                               |
| `keep-running.e2e.mjs`          | 启用后台运行后退出保留 daemon，重新启动接管同一 PID；关闭后台运行后正常退出终止 daemon 并清理交接记录                                                                                                                                                                          |
| `server-listen.e2e.mjs`         | 监听地址失焦保存，重启应用设置，通配地址、固定端口及再次启动重连通过                                                                                                                                                                                                           |
| `browser-relay-network.e2e.mjs` | 真实 daemon、Electron Chromium 与测试 Relay 完成远端回环访问、200 KB 响应、HTTP(S)/WS(S) 代理选择和断线恢复                                                                                                                                                                    |
| `settings-rust.e2e.mjs`         | 19 个现有页面、发布说明离线回退、项目详情、Agent profile/Skills、system prompt、Workspace/terminal 设置保存、Provider 设置通过；无 renderer error                                                                                                                              |
| 真实中心 Hub-only 联调          | 远端旧 Relay control 明确为离线；desktop 恢复临时账号、发现远端 Host，经 Hub 完成真实 RPC、创建工作区和两轮会话，并从聊天输入框发送第三条消息、显示对应回复；中心收到 Agent 摘要及 creation/Timeline/attention，desktop IPC 收到相应事件；desktop 重启后恢复远端连接与相同历史 |

真实中心测试账号通过本轮生成的临时签名和 JWT 创建；中心、数据库只监听 loopback。
未使用用户现有账号或数据，也未部署生产服务。

## 构建与定向回归

| 检查                                                                                          | 结果                                                                                            |
| --------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `RUSTC_WRAPPER= cargo build --offline --locked -p daemon --bin daemon`                        | 通过                                                                                            |
| `npm run build:desktop-assets`、desktop `build:main`                                          | 通过；Electron 共享界面导出 5437 个模块                                                         |
| SDK/mobile/desktop `typecheck`                                                                | 通过                                                                                            |
| SDK `account-session.test.ts` 与 `src/hub`                                                    | 7 个文件、138 项通过，包含 Hub-only 发现与跨中心隔离                                            |
| Desktop `account-transport`、`account-hub`、`account-session`、`account-ipc`                  | 4 个文件、33 项通过，包含缺失/过期旧实例下的 Hub 连接、错误 Host/实例/协议拒绝及既有 Relay 回退 |
| Mobile `account-state.desktop/channel/native`、`hub-account-transport`、`browser-account.web` | 5 个文件、42 项通过，包含 Hub-only Host 连接资格                                                |
| daemon `host::hub::`                                                                          | 20 项通过                                                                                       |
| daemon process `unix::metadata::`                                                             | 1 项通过，验证真实二进制重启后的项目持久化                                                      |
| daemon process `unix::terminal::`                                                             | 8 项通过，验证真实 PTY、订阅、断线恢复和收尾输出                                                |

Vitest 使用 `NODE_OPTIONS=--no-experimental-webstorage`，避免本机 Node 26 的实验性
global storage 覆盖 jsdom。只运行变更及直接相关测试，没有运行全量 Rust workspace 测试。

## 尚未完全排除的启动风险

补充聊天页重启复测时出现一次 `Rust daemon handshake timed out`。
日志确认 daemon 已 ready，但 `daemon.get_status.request` 直到退出才以 ServerDraining 收尾。
[状态 RPC](../../../crates/api/src/connection/dispatch.rs) 会等待 `provider.available.list.request`
完成，因此本机 Provider 目录探测慢时可能超过 desktop 的 30 秒启动预算。
这是当前代码中的等待关系；本轮没有锁定具体哪个 Provider 导致了这次超时。

仅将 desktop 的 Codex 可执行文件切换为同一离线 fixture 后，包含界面发消息和重启的完整
联调通过；其余 Provider 仍由本机发现。此前本地启动及 Hub 重启流程也已通过。
本次没有修改 Provider 探测/就绪逻辑，不能据此声称所有本机 Provider 环境都不会出现启动超时。
后续可单独构造慢 Provider 场景，验证就绪握手是否应与目录发现解耦。

## Test coverage

**not measured**：本轮未请求覆盖率测量，没有采集行覆盖率或覆盖率制品；如后续请求，按相关 crate 范围测量并附报告。上述计数是测试用例数。
APNs/FCM 真机送达、生产中心网络、真实模型供应商认证及推理未在本轮验证。
真实中心已验证事件持久化、账号订阅和 desktop 收取事件；它不等同于移动推送实际送达。
