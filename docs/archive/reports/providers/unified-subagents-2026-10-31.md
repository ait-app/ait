# 统一子 Agent 展示：实施与验证

日期：2026-10-31。基于提交 `19a829a462919c31693eac2c5caac86c207defac` 的工作区变更。
决策见 [ADR-209](../../../decisions/providers/adr-209-observed-subagent-normalization.md)。

## 交付行为

Codex、Claude 已验证的原生子会话登记为稳定的只读 Ait Agent，与 Ait 受管子 Agent
共用目录、直接父级关系、标签页和普通时间线接口。任务描述更新默认标题，手动改名保留。
嵌套发现、重复事件、重启和未完成登记复用同一 ID；已有 managed 身份保留原执行与流归属。

Timeline schema 7 为原有子会话 scope 增加宿主映射，升级保留 epoch、序列和历史。
同一条目向普通流和兼容流投影；活动局部历史只追加，明确完成的历史才允许 reconcile。
父前台 turn 结束后的后台子任务继续独立更新。观测 revision 防止旧目录响应覆盖终态。
重启、断线显示 stale；原根不可用时保留缓存，不能据此合成失败或成功。

服务端与客户端均限制操作：observed 没有输入框、取消、配置、独立恢复、refresh、脱离或
审批应答；定时任务排除它。终态收起仅更新 Ait 归档，原生日志保留。根归档保留原生父子关系。
新版 SDK 根据能力为目录与历史列表启用 observed；旧客户端继续使用原有子列表/事件。
旧标签页目标在新版 Host 上解析到普通 Agent 标签页，旧 Host 的兼容面板/store 保留。

## 自动验证

以下测试均为变更代码及直接相关行为的聚焦运行。部分 scope 有重叠，不把重复运行相加
作为测试总数。原生交互使用仓库内离线 Python 协议夹具，没有发起真实模型调用。

| 范围                        | 命令或筛选                                                                             | 结果               |
| --------------------------- | -------------------------------------------------------------------------------------- | ------------------ |
| Domain 状态                 | `cargo test --locked -p domain --lib agent_runtime`                                    | 3 通过             |
| 文件 registry 不变量        | `cargo test --locked -p persistence --lib storage::agent_runtime`                      | 5 通过             |
| Provider 管理、恢复、登记   | `cargo test --locked -p provider --lib service::agent_manager`                         | 109 通过           |
| Timeline / schema 升级 / 流 | `cargo test --locked -p provider --lib storage::timeline`                              | 46 通过            |
| 控制与统一子树离线集成      | `cargo test --locked -p provider --lib service::agent_execution::tests::controls`      | 10 通过            |
| 订阅与重连                  | `cargo test --locked -p provider --lib service::agent_execution::tests::subscriptions` | 20 通过            |
| Agent 元数据和归档          | `cargo test --locked -p provider --lib service::agent_runtime`                         | 31 通过            |
| Agent 目录 RPC              | `cargo test --locked -p provider --lib rpc::agent_runtime`                             | 7 通过             |
| Workspace 活动去重          | `cargo test --locked -p provider --lib service::workspace_attention`                   | 7 通过             |
| Codex 原生子发现            | `cargo test --locked -p provider --lib local::codex::controls`                         | 10 通过            |
| 子会话相关补充              | `cargo test --locked -p provider --lib subagents`                                      | 16 通过            |
| 定时任务目标过滤            | `cargo test --locked -p daemon --bin daemon host::schedule::tests`                     | 1 通过             |
| Mobile / Web 单元测试       | 下列 11 个直接相关文件                                                                 | 172 通过           |
| 协议                        | `messages.provider-subagents.test.ts`、`messages.wire-compat.test.ts`                  | 17 通过            |
| SDK                         | `daemon-client.test.ts` 中 unified Agent identity 与 fetchAgents timeout 用例          | 3 通过，167 未选择 |

前端聚焦文件：`subagents/{select,track-presentation,archive-finished,close-tab-policy}.test.ts`、
`utils/{agent-snapshots,agent-directory-update-policy,agent-directory-reconciliation}.test.ts`、
`runtime/rust-daemon/messages.test.ts`、`i18n/resources.test.ts`、
`schedules/workspace-schedule-form.test.tsx`、`schedules/schedule-form-model.test.ts`。
使用 `npm exec --workspace=@ait/mobile -- vitest run --project unit` 选择这些文件。
协议、SDK 分别使用对应 workspace 的 `vitest run` 并选择上述文件/用例。

新增集成断言包括：拒绝外部 ancestry；新旧目录隔离；旧目录游标切换；旧目标恢复；
普通与兼容时间线的 epoch/cursor 一致；readonly 请求不增加原生调用；父 turn 结束后
子任务仍运行并最终完成；终态收起不删除原生日志；重启后保留结果并标为 stale。

## 构建、格式与 lint

- `cargo check --locked --workspace` 通过，没有运行 workspace 测试。
- `cargo fmt --all --check`、变更 TS/TSX/Markdown 的 oxfmt、`git diff --check` 通过。
- `cargo clippy --locked -p domain -p persistence -p provider --lib -- -D warnings` 通过。
- `cargo clippy --locked --no-deps -p domain -p persistence -p provider -p daemon --all-targets -- -D warnings -A clippy::assert_is_empty` 通过。仅 lint 相关 target，保留本机 Rust 1.99 对既有测试新增的
  `assert_is_empty` lint 例外；生产 library 另按严格 `-D warnings` 检查。
  含依赖的 lint 还会遇到既有 schedule 单元素循环及 filesystem 弃用提示，本次未扩展修改这些代码。
- `npm run build --workspace=@ait/client`、`npm run typecheck --workspace=@ait/mobile`、
  `npm run check:ui` 通过。
- `npm run check:docs` 通过。
- Python 离线夹具通过 AST 语法检查，未生成运行产物。

构建可见既有 `crates/filesystem/src/workspace_runtime.rs` 的 Atomic `fetch_update`
弃用提示；本次未修改该无关代码。

## 未执行范围与限制

没有运行全 Rust workspace 测试、真实 Provider 模型调用或浏览器真实模型端到端测试。
浏览器相关用例已更新为普通 Agent 面板及副标题选择器，并通过 TypeScript 检查；这不等同于
已完成视觉或真实 CLI 验证。当前只接入已经提供可信子会话观测端口的 Provider，不根据工具名
推断其他 Provider 的子身份。

## Test coverage

未测量代码覆盖率，因此没有百分比或覆盖率 artifact。用户未要求全 workspace 测试或
覆盖率，本次按仓库规则运行变更与直接相关行为的聚焦测试。上表记录功能验证范围及结果，
不能用通过用例数推算代码覆盖率。
