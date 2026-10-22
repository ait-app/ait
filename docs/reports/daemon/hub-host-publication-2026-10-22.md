# Host Hub 发布接入验证

日期：2026-10-22。基线：`c8f89d0b`；实现对应本报告所在提交。
平台：macOS，本机 Rust 工具链；未执行其他操作系统或真机验证。
边界与切换方式见 [ADR-181](../../decisions/daemon/adr-181-hub-host-publication.md)。

## 改动结果

daemon 使用已有 Hub serve 连接主动调用中心的 `hub.events.publish` 和
`hub.directory.publish`。事件进入独立 journal 后，以稳定 event ID 重试，业务成功才确认。
目录发布三类轻量摘要；Timeline 仅发布持久历史更新标记。账号与 Host 来源由中心认证连接决定。

通知准备复用现有 Agent 状态验证，在连接焦点筛选之前进入发布流程。原生推送默认保留
旧客户端路径，客户端完成中心设备注册后，通过 `AIT_SERVER_HUB_NOTIFICATIONS=true`
启用中心原生通知。该模式下同一事件不同时进入 daemon 的原生投递 worker。

## 执行验证

以下全部通过。`RUSTC_WRAPPER=` 用于绕开当前环境的 sccache 沙箱限制；网络夹具只绑定临时
loopback 端口，文件夹具只使用隔离临时目录，未调用真实 APNs / FCM。

| 范围 | 命令（共同前缀 `env RUSTC_WRAPPER= cargo test --offline --locked`） | 通过 |
| --- | --- | ---: |
| daemon Hub，最终轮 | `-p daemon host::hub --bin daemon` | 20 |
| Hub 协议、桥接及续传，最终轮 | `-p relay hub:: --lib` | 31 |
| Host 生命周期及原生推送相关回归，较早轮 | `-p daemon host:: --bin daemon` | 48 |
| 启动配置 | `-p daemon config:: --bin daemon` | 13 |
| model 中心契约 | `-p model hub:: --lib` | 1 |
| 通用事件观察者 | `-p model events:: --lib` | 4 |
| 会话、焦点与通知路由 | `-p model session:: --lib` | 11 |
| 创建回执发布 | `-p model creation:: --lib` | 2 |
| 原子 journal adapter | `-p persistence storage::hub:: --lib` | 2 |
| 原生通知准备及旧投递 | `-p metadata push::delivery:: --lib` | 7 |
| Provider 自有订阅及发布观察 | `-p provider service::agent_execution::tests::subscriptions --lib` | 21 |
| Provider 持久 Timeline | `-p provider storage::timeline::tests --lib` | 9 |
| API Hub 控制及发布接线 | `-p api tests::hub_ --lib` | 3 |
| API 直接相关会话回归 | `-p api tests::paseo --lib` | 23 |
| 依赖边界 | `-p daemon --test dependencies` | 16 |
| metadata 模块边界 | `-p metadata --test module_boundaries` | 2 |

表中范围有重叠，通过数量不能直接相加。最终新增 Host 端到端夹具验证完整生产组装：
预置一个有效 attention，启动 daemon、下发 serve grant，模拟中心收到原生通知事件和三类
目录调用并返回业务结果；发布内容不包含 Provider 私有配置，停机刷新 journal。

关键协议验证包含：网络 ACK 后调用仍等待业务 reply、物理续传不会重复已接纳调用、新逻辑
链路使旧调用失败以便幂等重试、中心调用与原有业务会话共享序号、旧中心不收到未知调用、
调用数量和字节预算、存储失败及重新打开、来源范围切换、迟到确认、目录 revision 与空快照。

工程检查：

```text
env RUSTC_WRAPPER= cargo build --offline --locked --workspace
env RUSTC_WRAPPER= cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --check
```

未运行完整 Rust workspace 测试；构建和 lint 的 workspace 范围不执行全量测试。

## Test coverage

**not measured**：本次未请求覆盖率测量。上述通过数量是测试执行结果，不代表行覆盖率。
未生成或复用覆盖率百分比、基线差值和覆盖率制品。如后续请求测量，应使用
`cargo llvm-cov`，限制在本 PR 的相关 crate 和上述测试筛选范围，并提供可共享报告。

## 尚未验证及交付边界

- 当前验证使用模拟中心，未在本次 daemon PR 中运行真实 ait-server 数据库或真实设备推送。
- 客户端的 Hub `call/reply/event`、账号级设备注册、前台心跳、消费游标与跨 Host 导航不属于
  本 PR；开启中心原生通知前必须完成这些接入，并在中心配置提供方凭据。
- 业务提交与 ingress 入盘不处于同一事务。正常停机刷新队列，最近 Agent attention 可从
  registry 恢复；崩溃或入口溢出时，尚未入盘的临时 terminal hook 无法恢复。
- 永久拒绝会保留隔离记录；中心事件保留窗口及原生提供方投递仍遵循中心端的限制。
