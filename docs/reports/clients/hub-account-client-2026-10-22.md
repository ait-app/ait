# Hub 账号客户端接入验证

日期：2026-10-22。平台：macOS，Node.js 本机环境。基线：`a389b868`；实现对应本报告所在提交（PR #447）。

## 范围

实现决策见 [ADR-182](../../decisions/clients/adr-182-hub-account-client.md)。
本次增加 SDK 的中心调用、账号事件补拉，桌面和托管浏览器的 Hub 优先会话，
账号级原生设备登记、心跳、多窗口兴趣清理和通知去重。沿用现有单账号及主机列表，
没有新增页面、导航入口或账号层级。Hub Agent 摘要仅作为通知内部数据。

## 验证

- SDK：Hub 帧、中心调用、会话、物理续传、ACK 不完成业务调用、调用预算、游标分页及重建订阅。
- 桌面：真实本机 WebSocket 的既有 Relay 回退，Hub hello/accept 与 Host 身份校验，
  续传、窗口销毁、订阅归属、跨窗口前台汇总与通知去重。
- 共享界面：账号状态同步、原生推送权限与 token 轮换、账号注销、目录分页、同 ID 多 Host、
  通知来源和清除 attention 筛选；确认通知流程不读取无用途的项目及工作区摘要。

定向测试共 145 项通过，未重复累计重跑用例：SDK Hub 44 项、桌面 21 项、共享界面与账号/推送 61 项、通知路由与系统通知 19 项。

| 命令                                                                                                  | 结果                                          |
| ----------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| `npm exec --workspace=@ait/client -- vitest run src/hub`                                              | 6 个文件，44 项通过                           |
| 桌面 `vitest run`：`account-hub`、`account-transport`、`account-ipc`、`notification-dedup`            | 4 个文件，21 项通过                           |
| 移动端 `vitest run`：账号状态三组、浏览器账号、Hub/桌面传输、推送登记、通知摘要及两个现有账号界面文件 | 10 个文件，61 项通过                          |
| 移动端 `vitest run`：`notification-routing`、`os-notifications`                                       | 19 项通过                                     |
| `npm run typecheck --workspace=@ait/client`、`@ait/mobile`、`@ait/desktop`                            | 全部通过                                      |
| `npm run build --workspace=@ait/client`                                                               | SDK 与协议构建通过                            |
| `npm run check:ui`、`npm run check:format`、`npm run check:docs`、`git diff --check`                  | 全部通过                                      |
| `expo export --platform web --output-dir /private/tmp/ait-hub-account-web`                            | 共享 Web 界面导出通过，未包含额外资源浏览路由 |

Vitest 使用 `NODE_OPTIONS=--no-experimental-webstorage` 避免本机 Node 实验性全局 storage 覆盖 jsdom storage。
依赖通过 `npm ci` 按锁文件安装，并应用仓库既有补丁；未修改依赖或锁文件。

## Test coverage

本次未更改 `bins/` 或 `crates/` 中的 Rust 代码，按仓库规则未运行 Rust 测试。
未采集覆盖率百分比。测试只覆盖本次变更及直接相关行为，未运行全量 workspace 测试。
未在本次验证中执行真机 APNs/FCM 投递或部署中心；该验收需要中心提供方凭据及支持账号方法的服务。
