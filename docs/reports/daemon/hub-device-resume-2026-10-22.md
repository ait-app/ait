# Hub 真机互通与端点续传验证

- 日期：2026-10-22；PR：[#438](https://github.com/ait-app/ait/pull/438)。
- 真机测试源码：ait `5ef766a13f70422bd339b4445f271ae7284885cb`；ait-server `0163494`。
- 本次 rebase 基线：`upstream/main` 的 `624bf3df5593e74fddc08f64a3cafb5e534afdb5`；重放后的功能提交为 `0160fdf0`。
- 决策：[消息 Hub 服务端点](../../decisions/daemon/adr-180-hub-serve-endpoint.md)。

真机实测发生在 rebase 前。Rebase 仅手工合并文档索引冲突，Hub 实现和相关测试内容保持一致；下文另列 rebase 后的检查，不能将已有 APK 或运行中的容器视为已重新构建的 rebase 版本。

## 环境与方法

Pixel 9 Pro（Android 17）安装本分支 arm64 调试 APK `dev.ait.mobile.debug`，通过 Metro 加载同一提交的 JS。Linux Docker 中运行真实中心、独立 PostgreSQL、Electron Desktop 和独立 Rust daemon。Desktop 使用已构建的主进程及 renderer assets，在 Xvfb 中启动自己的 daemon。

使用隔离账号和数据库。测试账号经 Hermes 调试器交给实际账号管理器，后续认证、业务请求和订阅均走真实中心。独立 daemon 使用外部授权模式，测试控制器负责租约与 Hub/旧 relay 控制票据维护；没有替换业务服务实现。

手机经 USB 端口转发连接测试中心。故障代理可丢弃数据或切断指定链路，保留其他连接。观察项包括：

- Hub link ID、逻辑 session ID，以及 `hello.resume`、`welcome.resumed`、`recv`、`seq`。
- 业务客户端对象、连接 epoch/generation、订阅 ID、订阅推送计数和错误。
- 故障期间发出请求的完成状态，以及恢复后的真实项目列表、工作区和终端操作。

确认两台 Host 都有 serve presence，手机日志均选择 `path: hub`，再执行正式故障用例。

## 手机 access 端点

下表均同时覆盖 Desktop 内置 daemon 和独立 daemon。

| 场景                                                      | 结果                                                                 |
| --------------------------------------------------------- | -------------------------------------------------------------------- |
| 后台 10 秒再返回前台                                      | 原 link、session、客户端连接代次和订阅 ID 均保持，RPC 成功           |
| 后台期间静默断网，回前台 4 秒后恢复                       | 触发超过 3 秒的探活恢复；原业务会话和订阅继续使用                    |
| 前台连续 30 秒双向黑洞，不主动发送 close/error            | 心跳发现失联并续传；恢复网络后约 1.5 秒重新 online，原会话和订阅保持 |
| 连续 3 次切断 access 连接，断线期间分别向两台 Host 发请求 | 6 个请求均补发成功；最终一轮从发送到收到结果为 107–603 ms            |
| 恢复后创建真实终端                                        | 两端原订阅均继续收到列表推送，错误列表为空                           |

## Host serve 端点

为两台 Host 的 serve 连接分别设置代理，手机 access 连接保持在线。切断 serve 约 8.5 秒，期间手机向每台 Host 发出项目列表请求。

- 两台 Host 在新物理连接上发送携带原 link ID 和已接收序号的 `hello.resume`。
- 中心返回相同 link ID，且 `welcome.resumed = true`。
- 中心排队的请求在续传后得到响应。Rust 重连受退避影响，本次请求约 15 秒完成；没有以固定 3 秒作为恢复期限。
- 手机 access link 始终 online；两个 session、业务客户端对象、连接 epoch/generation 和订阅 ID 均不变。
- 恢复后两端原订阅继续收到真实终端创建推送。

另一个用例在代理上丢弃一条 Host 已发送、中心尚未收到的响应，立即断开物理连接，逐帧核对恢复过程：

| Host                | 被丢弃的 `seq` | 续传 welcome 的 `recv` | 恢复后重发 `seq` | session    | 结果     |
| ------------------- | -------------- | ---------------------- | ---------------- | ---------- | -------- |
| 独立 daemon         | 18             | 17                     | 18               | 原 session | RPC 完成 |
| Desktop 内置 daemon | 26             | 25                     | 26               | 原 session | RPC 完成 |

两台 Host 的 welcome 均为 `resumed: true`。响应以原序号和原 session 重发，原业务会话与订阅保持，不需要重新执行业务 hello。

## 其他端到端行为

- 手机对两台 Host 的项目列表 RPC、打开隔离工作区、创建/关闭真实终端均通过。终端订阅分别收到创建和关闭更新，无订阅错误。
- 中心重启后内存中的链路状态丢失，预期重新建链。本次约 2.8 秒恢复两台 Host 的新 link/session 和项目列表 RPC；此结果不计为同一端点续传。
- Host 初次上线而 serve 尚未就绪时，观察到 `legacy:no_serve` 回退。确认 serve 就绪并重新建立访问会话后，才将结果计入 Hub 用例。

## 测试环境修正与证据保存

独立 daemon 的最小镜像最初缺少 git，补齐后工作区和终端操作通过。外部授权夹具最初只签发一次旧 relay 控制票据，中心重启后 Host 可用状态没有恢复；补齐票据维护后重测通过。它们是测试环境调整，没有修改产品代码。

首次 Host 续传用例仅在恢复网络 3 秒后读取结果，早于 Rust 下一次退避重连，因而尚有请求未完成。改为在有界时间内等待实际恢复后通过。此修正没有缩短产品的重连退避。

原始脚本、脱敏日志和结果留在执行机器的工作区外置 `artifacts/hub-device-20261022/` 目录。关键文件为 `fault-results.json`、`extra-results.json`、`center-restart-results.json`、`terminal-results.json`、`serve-resume-results.json`、`serve-replay-proof.json` 和 `serve-frames.jsonl`。该目录不是仓库内共享产物，本报告保留可供审阅的结果和序号证据；凭据、数据库、APK、运行日志不提交到 Git。协议头记录已排除票据、resume token 和业务 payload。

测试结束后代理恢复 online，测试终端与当前测试订阅已清理。

## Rebase 后检查

在 `624bf3df` 基线上重放完成后执行：

| 检查                                                                                          | 结果                 |
| --------------------------------------------------------------------------------------------- | -------------------- |
| client：`vitest run src/hub src/account-session.test.ts src/daemon-client.test.ts`            | 6 个文件，297 项通过 |
| mobile：`vitest run --project unit`，限定 Hub 路径、Rust transport 和 native account 三个文件 | 3 个文件，76 项通过  |
| `npm run build:sdk`                                                                           | 通过                 |
| mobile、desktop `npm run typecheck`                                                           | 均通过               |
| `cargo check --locked -p relay -p api -p host-link -p daemon`                                 | 通过                 |
| 文档链接检查、修改文档的 oxfmt 检查、`git diff --check`                                       | 通过                 |

合计 373 项定向 TypeScript 测试通过。核对 rebase 前后 Hub Rust、共享客户端、移动端 runtime 与桌面 daemon 目录的内容一致；本轮没有新增 Rust 改动，未重跑 Rust 测试或完整 workspace 测试。

## 验证边界

本报告不代表生产公网或运营商切网验收。以下场景没有在本轮真机验证：Android release APK、iOS、macOS/Windows、推送通知、OAuth 登录、第三方 AI 提供方、headless 机器凭据自管理授权、5 分钟 replaced 退让、超出宽限期、凭据吊销与恶意续传参数。

本轮没有重新测量覆盖率，之前版本的覆盖率不能作为 rebase 后的测量结果。
