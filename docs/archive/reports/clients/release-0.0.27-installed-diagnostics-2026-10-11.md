# Ait 0.0.27 安装后诊断与修复

日期：2026-10-11。安装版检查对应发布标签 `v0.0.27`，提交 `1fa30380`；
修复已同步至 `main` 的 `be27dcdc`，包含 Workspace RPC 迁入 metadata 的重构。
平台：macOS；检查正式安装的桌面应用、内置 daemon、应用诊断导出与开发者控制台。
修复范围以本报告所在提交为准。

## 安装与运行状态

桌面应用与内置 daemon 均为 `0.0.27`。本机 `/healthz` 返回 `alive`，`/readyz`
返回 `ready`；正常诊断导出显示内置主机在线。检查期间未发现本次安装后的 daemon panic
或应用崩溃报告。更新已完成，以下问题是运行时协议、订阅时序和诊断行为的问题。

## 发现与修复

1. **原生用量事件被丢弃。** 控制台中的校验错误指向
   `message.payload.event.type` 与 `message.payload.timestamp`。Rust 发布
   `usage_updated`，但客户端判别联合未定义该事件，发送端也未提供时间戳。
   补齐用量事件 schema，保留用量字段；Rust 的开始、用量和权限事件均发送时间戳。
   客户端对旧 `0.0.27` 主机缺少时间戳的这四类控制事件使用接收时间，保留新主机的
   源时间戳，并继续拒绝畸形用量、未知事件和缺少时间戳的 Timeline 行。
2. **后台繁忙导致订阅永久失败。** 控制台确认
   `workspace.label.list.request` 返回 `resource_exhausted`。Workspace 目录和标签
   订阅已被连接接纳，初始化却再次尝试抢占单个共享后台任务名额；客户端有限重试耗尽后
   释放订阅。初始化改用现有 `run_queued` 等待同一个任务预算；连接订阅上限和后台并发
   限制保持原值。回归测试持有任务名额，验证等待期间没有错误响应，释放后返回带有
   连接所有权的快照。
3. **握手前订阅了未确认支持的事件。** 控制台确认
   `session.events.set_subscription.request` 返回 `unsupported_capability`。
   SessionProvider 的效果在握手前捕获未过滤的事件集合，SDK 重连恢复也可能重用旧集合。
   现在通过 `observeEvents(..., { supportedOnly: true })` 在本地登记监听，由 SDK
   在每次握手后按当前主机公告生成事件需求。过滤在建立或恢复订阅时执行，兼容旧订阅
   协议；主机未公告事件列表时保留请求集合。其他调用方默认行为不变。
4. **更新成功后的缓存路径误报。** ShipIt 保留的状态仍指向已移入 Applications 的暂存
   bundle；诊断调用 `plutil` 读取已不存在的 `Info.plist`，误报版本读取失败。
   文件不存在时返回未知暂存版本，其他读取错误仍保留为诊断错误。

本次沿用已有组件所有权、订阅协议与任务预算，没有改变领域依赖边界。

## 验证

- 原生协议适配器消息与传输测试：68 项通过，含真实 SDK 消费用量和权限事件的回归。
- SDK 连接测试：40 项通过，含握手前登记、重连能力变化、空能力列表、旧主机兼容
  与握手前释放；本次新增 7 项订阅协商测试和 5 项流事件消费测试。
- 关联协议解析与 Agent 快照测试：26 项通过；更新器诊断测试：6 项通过。
- TypeScript 定向测试合计 140 项；更新器暂存路径回归不依赖 macOS 的 `plutil`，
  可在 Linux CI 执行。
- Rust 原生用量/权限测试、目录同步测试、标签测试与 API 分发测试：53 项定向通过。
  已补齐同步 main 后目录订阅测试夹具新增的服务字段。
- daemon、SDK 和桌面主进程构建通过；SDK、桌面与移动端 TypeScript 类型检查通过。
- 修改文件 Oxfmt/Oxlint、Rust fmt、相关 crate 严格 Clippy 与补丁检查通过。

Rust 定向测试使用 `cargo nextest run --locked`，先对 `provider`、`metadata`、`api`
选择以下测试，再单独补查 metadata 的标签协议和 RPC 测试：

```sh
cargo nextest run --locked -p provider -p metadata -p api \
  -E 'test(service::agent_manager::tests::usage::) | test(service::directory::tests::synchronization::) | test(service::workspace_labels::tests::) | test(connection::dispatch::tests::)'
cargo nextest run --locked -p metadata \
  -E 'test(workspace_labels) & not test(service::workspace_labels::tests::)'
```

检查使用真实安装版；修复验证使用隔离源码工作区和测试中的本地资源。
已安装的正式应用仍为原 `0.0.27`，这些源码修复需要打包并更新客户端才能生效。
未在运行中的用户会话里替换程序或重启 daemon，也未进行正式发布成品验证。
原始诊断报告留在本机，没有纳入源码或上传。

## Test coverage

**Not measured。** 用户未要求覆盖率测量，本次按仓库规则仅执行修改代码与直接相关行为
的定向测试，没有运行全 Rust workspace suite。测试通过数量不代表行覆盖率。
若后续需要测量，使用 `cargo llvm-cov` 对 `provider`、`metadata` 和 `api` 的上述
定向测试生成可共享报告；安装包更新后的真实控制台与启动订阅仍需成品复查。
