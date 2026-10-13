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
