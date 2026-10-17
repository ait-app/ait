# Relay 连接加速验证

日期：2026-10-13。分支 `perf/relay-connect-latency`，实现验证版本 `de8880d8`，
基于主分支 `13ce800c`；验证后仅更新本报告。
范围为 Host 连接复用与恢复、Host 侧 TLS 会话缓存、TCP_NODELAY、Relay 客户端重连退避。
未修改中心协议、票据权限、配对/hello 超时或领域边界。

## 实现与验证范围

- 保留启动时预连接及页面切换复用；账户发现恢复时唤醒已有主机，同一批在线主机刷新不重复唤醒。
- 首次探测失败、没有活动 client 时，恢复事件立即重试；并发事件合并，失败的活动 client 可以替换。
- Web/Electron 在前台收到 `online` 时沿用立即重连/探活逻辑；原生端沿用 AppState 和账户恢复通知。
- Relay 重连从 500 ms 起步，连续失败指数退避，上限 30 秒；一次性探测仍关闭自动重连。
- Rust 控制、数据和下载 WSS 共用 TLS 配置与会话缓存；出站 WSS 和本机 WS 开启 NODELAY。
- 本机动态生成测试证书，验证第一次完整 TLS 握手、第二次会话恢复，以及两种连接的 NODELAY。
  默认信任配置仍拒绝自签名证书；没有将测试证书、私钥或运行凭据保存进仓库。
- 连接配置测试改用静态导入，避免将整个应用模块的首次转译计入单个行为用例的 5 秒超时。

## Test coverage

行覆盖率 **not measured**：本次未请求覆盖率测量，没有覆盖率百分比或覆盖率产物。
如后续需要，使用 `cargo llvm-cov` 对 `relay` crate 定向测量，并单独测相关客户端文件。
未运行完整 Rust workspace 测试。

测试执行结果与覆盖率分开：Linux 本机，Rust 默认 features，以下定向测试通过。
Rust 网络用例在允许监听回环端口的环境运行；外部服务、客户端传输与账户 API 使用测试替身。

| 命令                                                                                                                                                                                                                                                                                                          | 结果                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| `cargo test -p relay --locked`                                                                                                                                                                                                                                                                                | 22 项通过，0 个 doctest |
| `npm run test --workspace=@ait/client -- src/account-session.test.ts src/daemon-client.test.ts`                                                                                                                                                                                                               | 218 项通过              |
| `npm run test --workspace=@ait/mobile -- --project unit src/runtime/host-runtime.test.ts src/runtime/account-state.desktop.test.ts src/runtime/account-state.native.test.ts src/navigation/host-runtime-bootstrap.test.ts src/utils/test-daemon-connection.test.ts src/runtime/native-account.native.test.ts` | 150 项通过              |

没有测量线上冷连接/重连 p50、p95 或实际加速百分比。TLS 会话恢复测试证明缓存能够复用，
不证明线上 TLS 终止代理已启用会话恢复。Android/iOS 安装包后台恢复、真实网络切换及代理配置
仍需实机验收；原生端没有新增 OS 网络监听，应用保持前台时依赖账户恢复和正常退避。

## 构建与静态检查

- 初版通过 `cargo check --workspace --locked`、`cargo clippy -p relay --all-targets --locked -- -D warnings`。
- 更新主分支基线后，`cargo build --workspace --locked`、`cargo clippy --locked --workspace --all-targets -- -D warnings` 通过；上述 390 项定向测试重新通过。
- `cargo fmt --all --check`、`git diff --check` 通过。
- `npm run build:sdk`、`npm run typecheck --workspace=@ait/client --workspace=@ait/mobile` 通过。
- 修改的 12 个 TypeScript 文件通过 `npm exec -- oxlint <文件列表>`；修改的 TypeScript/Markdown 文件通过 `oxfmt --check`。
- `npm run check:docs` 通过。

## 2026-10-18：客户端恢复等待优化

本轮基于 `3be1286f`，修改共享 TypeScript 客户端，覆盖 Android、iOS 与 desktop。

- 显式恢复时，已等待至少 3 秒的业务握手立即重试；刚开始的尝试保留，旧 transport 的 open/error/close 回调不能干扰替换连接。普通 45 秒连接期限保持不变。
- 状态 Channel 在前台恢复时使用原有心跳，3 秒无响应后立即重建；健康连接保留，重复检查不延长期限，退出会取消检查。正常状态心跳仍遵循服务端参数。
- 账户维护期间收到的恢复信号合并到当前任务完成后执行，消除被普通 2 秒轮询覆盖的等待。
- 已有租约过期后立即注册一次；注册失败或新租约持续被拒绝仍退避，避免连续快速注册。
- 不增加协议字段、服务端能力或原生依赖，不依赖 Host 持久 WSS/Yamux PR。主分支已有的 `ait-channel-v1` 服务端要求保持不变；未增加对更旧中心的协议回退。

定向测试共 **441 项通过**：共享 SDK 280 项、移动端及共享界面 152 项、desktop 9 项。
用例覆盖旧 transport 回调、重复恢复、健康连接保留、租约失败退避、后台恢复与退出。
Android/iOS 使用生命周期和网络测试替身；desktop 状态适配器使用本机真实 WebSocket 验证心跳认证和静默恢复。

SDK 构建、client/mobile/desktop 类型检查、修改文件的 oxlint/oxfmt、文档检查和 diff 检查通过。
本轮 **Test coverage：not measured**，未运行覆盖率测量；没有 Rust 修改，未运行 Rust 测试。
3 秒为客户端存活检查期限，不是端到端重连承诺。未测线上 p50/p95 或真机网络切换；
原生端前台期间的 OS 网络监听和首次探测仍沿用原有机制。
