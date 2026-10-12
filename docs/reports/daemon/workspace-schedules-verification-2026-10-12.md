# Workspace 定时任务设计与验证

日期：2026-10-12。验证对象为基于 `491dc32888075cd314cdbe891ead5fe2c111ce3d` 的开发阶段工作区变更。
macOS 本机执行，浏览器为 Playwright Chromium；执行使用仓库离线 Codex stdio 协议夹具，
经过 Rust daemon、原生 provider 适配器与真实持久化，不调用模型服务。

设计依据：[ADR-138](../../decisions/workspace/adr-138-workspace-owned-schedules.md)。
使用方式：[Workspace 定时任务](../../operations/schedules.md)。

## 完成的行为

- Workspace 新建菜单在拉取请求后新增定时任务，以 Workspace 标签页创建、查看和管理。
- 新建要求固定 `workspaceId`；可以选择该工作区内未归档 Agent，或每次在该工作区创建 Agent。
- 支持单次、真实固定间隔和带 IANA 时区的 Cron，以及首次触发、截止时间、提示词和最大次数。
- 提供暂停/恢复、手动运行、编辑、删除、运行历史、输出展开和 Agent 跳转。手动运行期间可暂停后续触发。
- 创建后的详情与工作区任务列表身份随标签布局恢复；关闭标签不删除任务。
- 新 Agent 的归档只影响该 Agent；正常结算、失败清理和重启恢复都保留所属 Workspace。
- 版本 1 旧记录保留未绑定状态和旧执行方式，明确编辑选择 Workspace 后绑定；版本 2 记录新归属。

## 端到端证据

Workspace 的新建菜单打开表单，客户端提交包含 `workspaceId` 的创建 RPC。schedule actor
在有界异步校验中调用宿主 port 检查工作区和 Agent 成员关系，校准实际目录，然后原子写入
任务文件。任务标签转为详情；手动或自动触发通过 provider 执行，分配身份在启动 turn 前
持久化。结算后详情读取实际 `run.workspaceId`、Agent、状态与输出。

6 项浏览器用例全部通过：

1. 已有 Agent：排除外工作区 Agent，拒绝小数次数，创建、暂停、编辑提示词、手动运行、
   输出展开、页面重载、列表重载、次数耗尽与确认删除。
2. 新 Agent：单次任务创建、执行、终止、归属工作区保留和重载恢复。
3. 390 × 844 窄屏：全局入口选择 Workspace、选择已有 Agent、创建与关闭下一次创建表单。
4. 编辑已有任务恢复模型、思考档位、目标 Workspace 和 Cron。
5. 第二个隔离主机使用独有模型，重载后恢复所属主机的模型和运行模式。
6. 关闭编辑后重新创建，工作区、模型和运行模式不沿用上一次编辑。

检查了桌面和窄屏截图：详情、运行状态、历史和窄屏表单可见，控件未溢出。截图及服务端
运行日志保留在临时目录，没有写入源码仓库。用例收集浏览器 console 和 pageerror；最终
运行未报告响应校验失败，Metro 和隔离 daemon 正常停止。

验证期间修正了无时区 Cron 返回 null 与协议 optional string 不一致的问题，并增加 Rust
回归测试；统一弹层容器避免选择 Workspace 时表单重挂载。旧 mock provider 用例已换为
当前 Rust daemon 的离线原生协议夹具。

## 定向测试和检查

| 范围                       | 命令                                                                                                | 最终结果                               |
| -------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------- |
| 调度引擎、服务和文件边界   | `cargo test -p schedule --lib`                                                                      | 80 通过                                |
| 任务文件版本与原子写入     | `cargo test -p persistence --lib storage::schedule::`                                               | 4 通过                                 |
| daemon 定时任务进程集成    | `cargo test -p daemon --test process schedule:: -- --nocapture`                                     | 7 通过                                 |
| 前端相关模块               | 下方 Vitest 命令                                                                                    | 17 文件、242 通过                      |
| SDK 定时任务传输           | `npm exec --workspace=@ait/client -- vitest run src/daemon-client.test.ts -t 'schedule\|schedules'` | 5 通过，142 不相关用例跳过             |
| 协议 schema                | `npm exec --workspace=@ait/protocol -- vitest run src/schedule/rpc-schemas.test.ts`                 | 4 通过                                 |
| 浏览器完整操作链           | 下方 Playwright 命令                                                                                | 6 通过                                 |
| Cargo workspace 类型与依赖 | `cargo check --workspace`                                                                           | 通过                                   |
| Rust lint                  | `cargo clippy -p schedule -p persistence -p daemon --all-targets -- -D warnings`                    | 通过                                   |
| Rust 格式                  | `cargo fmt --all -- --check`                                                                        | 通过                                   |
| SDK 与 UI 依赖构建         | `npm run build:ui-deps`                                                                             | 通过                                   |
| Mobile、SDK 类型           | `npm run typecheck --workspace=@ait/mobile`、`npm run typecheck --workspace=@ait/client`            | 通过                                   |
| 变更文件 lint              | `npm run check:ui`                                                                                  | 0 错误、0 警告                         |
| Mobile 变更文件 ESLint     | 按变更文件列表运行 ESLint                                                                           | 0 错误；3 个 HEAD 已存在的未用变量警告 |
| 格式、补丁空白             | 变更 TS/TSX/Markdown 的 `oxfmt --check`、`git diff --check`                                         | 通过                                   |
| 文档链接                   | `npm run check:docs`                                                                                | 通过                                   |

前端相关模块命令：

```sh
npm exec --workspace=@ait/mobile -- vitest run src/schedules \
  src/hooks/use-schedule-mutations.test.ts src/hooks/use-schedules.test.ts \
  src/screens/schedules-screen-state.test.ts src/utils/schedule-format.test.ts \
  src/utils/schedule-cadence-policy.test.ts src/workspace-tabs/identity.test.ts \
  src/stores/workspace-layout-store.test.ts src/workspace-tabs/launcher/internal/catalog.test.ts
```

浏览器命令：

```sh
npm run test:e2e --workspace=@ait/mobile -- \
  e2e/browser/schedules-project-target.spec.ts \
  e2e/browser/schedules-edit-model-hydration.spec.ts
```

Rust 进程与浏览器测试需要本机监听端口；在授权的隔离本机环境执行。未运行全 Rust
workspace 测试，也没有运行不相关的前端用例。

## Test coverage

**Not measured**：用户未要求测量覆盖率；测试执行数量不代表覆盖率。没有与本次变更可比
的覆盖率基线或覆盖率 artifact。若后续要求测量，应仅对 schedule、persistence 的相关
模块和 daemon 定时任务路径执行定向测量，另行报告 revision、covered/total lines 和 artifact。
全 workspace 测量需另行明确授权。

未验证真实模型服务、Electron 成品、iOS/Android 真机，以及长时间 daemon 离线的真实钟表
场景；自动触发和重启恢复已通过离线协议进程集成验证。本次验证不声称覆盖上述平台。
