# Ait 0.0.28 客户端告警修复与诊断验证

日期：2026-10-11。平台：macOS arm64。源码基线：`8cdeff9baac82495e1cef45c3cb9752c66a45a28`，修复分支：`codex/fix-client-warnings-0-0-28`；修改范围以本报告所在提交为准。

## 安装版证据

检查正式安装的 Ait `0.0.28`、内置 Rust daemon、开发者控制台及应用自带诊断导出。实际日志为 `~/Library/Logs/Ait/main.log` 与 `~/.ait-server-desktop/daemon.log`。

当日大量 `Message validation failed [object Object]` 最后出现于 18:50；`0.0.28` 客户端于 18:56 启动。这些校验告警属于之前的进程，原生控制事件兼容修复已经进入该发布版。没有将旧日志计为新补丁失败，也没有清空历史证据。

`0.0.28` 启动时仍有四条界面 warning：Web 推送令牌监听、两个已移除的 Plugin 路由、原生动画驱动回退；同时有三次缺少细节的订阅错误。正式 daemon 本轮启动后记录了 19 次 `ResourceExhausted` 和一次 `AgentNotFound`。DevTools 自身的 Autofill 协议报错发生于打开开发者工具时。

## 修复

1. 已进入 API 有界队列的普通 RPC 改为等待共享短任务许可。保留执行并发、超量拒绝、组内顺序和停机取消语义；不重放操作。持有共享许可的回归在旧实现立即失败，新实现等待并在许可释放后成功。长期决策见 [ADR-134](../../decisions/daemon/adr-134-admitted-rpc-job-waiting.md)。
2. 原生通知模块仅在 Native 分支按需加载；取消后的异步回调不再导航，监听器随效果清理。桌面/Web 不再触发原生推送模块的导入副作用。
3. 移除已不存在的 Plugin 页面声明。两个骨架动画组件仅在 Native 使用原生驱动。
4. 目录同步和 Provider/配置更新的事件订阅在每次握手后按主机公告过滤。Rust 适配器补齐 Project 与脚本事件公告，避免完整主机丢失这些更新。缺少公告的旧主机继续沿用 SDK 兼容行为。
5. SDK 默认日志将错误序列化为有界文本，保留 Error 的类型、消息、栈、错误码与请求身份。订阅错误增加主机 ID、版本和请求的事件集合。协议校验告警增加消息类型、主机版本、大小与嵌套字段路径；不输出被拒绝的正文。敏感字段、认证字符串和 URL 凭证脱敏，循环引用与超长字段受到限制。
6. daemon 的 RPC 拒绝日志增加方法、请求 ID、通过协议校验的目标 Agent/Terminal/Workspace ID、剩余订阅容量和任务额度快照。事件拒绝记录受支持的类别与生产服务安装状态；协商失败记录是否安装、是否协商；队列超量记录能力组与容量。

三次订阅错误在原版的隔离正常启动流程中未复现，旧日志也没有保存足够的请求上下文。补齐遗漏的协商入口后，本机重新启动未再出现订阅错误；不能据此推断历史三次错误的全部原因。

## 本机应用验证

修复版 daemon、SDK 和桌面资源构建通过。保留原应用完整备份，使用相同 Developer ID 签名本地补丁，`codesign --verify --deep --strict` 通过；安装版与测试包内 daemon 的 SHA-256 一致。补丁仍显示版本 `0.0.28`，这是本地修复，未进行正式发布。

在确认 Agent 已结束后重启应用。首次补丁于 19:51 启动，观察至少五分钟，应用日志及 daemon 日志均为零新增 warning/error。Diff 刷新完成且连接保持正常。应用自带诊断确认主机在线、daemon `0.0.28` 正常运行、五个 Provider 可用，诊断错误为 `none`。随后将目标对象 ID 日志字段加入最终签名补丁并重新启动；最终进程于 20:03:59 就绪，至 20:08:59 再观察五分钟仍无新增告警。最终 `/healthz`、`/readyz` 均返回 HTTP 200，分别为 `alive`、`ready`；已安装最终 daemon 的隔离 SDK 集成再次通过。

原应用备份位于本机 `.tmp/daemon-warning-hotfix-20261011/original/Ait.app`。诊断导出与完整原生日志留在临时目录，没有纳入源码或上传。

## 剩余告警与信息保留

隔离测试分别注入未知事件、未协商的配置方法、缺失 Agent 的 Timeline 请求，确认错误仍返回，且日志能区分原因。例如受控测试产生：

```text
rpc{method="daemon.config.get.request" request_id="events-2" resource="capability_negotiation" installed=true negotiated=false}: RPC failed error_code=UnsupportedCapability
rpc{method=agent.timeline.get.request request_id=events-1 agent_id="diagnostic-missing-agent" ...}: RPC failed error_code=AgentNotFound
```

正常三组启动事件订阅在原版与最终补丁的隔离 daemon 中均成功。共享资源超量、业务对象不存在和实际能力不足仍可以产生告警；新增上下文用于查明具体拒绝位置与目标。

原生 OpenCode 的诊断窗口另捕获 133 条配置归一化告警，涉及旧模型字段 `attachment`、`reasoning`、`temperature` 与 `interleaved`。该 CLI 明确记录 `kind=unsupported`、完整配置字段路径以及忽略旧设置的动作；这些信息保留在应用诊断中。它们属于原生 CLI 配置诊断，不能计入上述 Ait 应用/daemon 日志的零新增结果。本次没有迁移第三方 CLI 的全局模型配置。

## 定向验证

- Rust `model`：Context 8 项、Runtime 13 项通过，涵盖共享许可等待、停机取消、已开始任务的追踪及各独立预算。
- Rust `api`：连接分发 21 项、事件生产服务 1 项、启动突发 2 项、单连接 2 项通过。合计 47 项不同 Rust 定向测试，没有运行完整 workspace suite。
- SDK：协议错误上下文、订阅身份、日志脱敏与边界测试共 151 项通过。
- 客户端目录同步、推送路由与 Rust 消息适配器：80 项通过。
- 最终签名包 daemon 与真实 SDK 的隔离集成：1 项通过，覆盖三组事件公告及订阅、目录、会话创建、Timeline、权限、恢复与终端；原生 Provider 使用离线测试夹具。
- Rust fmt、受影响 crate 严格 Clippy、修改文件 Oxfmt/Oxlint/ESLint、SDK 与移动端 TypeScript 类型检查、文档链接及补丁空白检查通过。
- PR 提交前另完成 workspace 构建及所有目标严格 Clippy 检查；沿用已通过的定向测试，没有扩大测试或覆盖率范围。

主要命令：

```sh
cargo test -p model context::tests --lib --locked
cargo test -p model runtime::tests --lib --locked
cargo test -p api --lib --locked connection::
cargo test -p api --lib --locked tests::session
cargo test -p api --lib --locked tests::chunks::startup_burst
cargo test -p api --lib --locked tests::single
cargo clippy -p model -p metadata -p api --all-targets --locked -- -D warnings
cargo build -p daemon --bin daemon --release --locked
cargo build --workspace --locked
cargo clippy --locked --workspace --all-targets -- -D warnings
npm run test --workspace=@ait/client -- src/diagnostic-log.test.ts src/daemon-client.test.ts
npm run test --workspace=@ait/mobile -- src/data/push-router.test.ts src/runtime/directory-sync/index.test.ts src/runtime/rust-daemon/messages.test.ts
AIT_TEST_RUST_SERVER="$PWD/.tmp/daemon-warning-hotfix-20261011/Ait.app/Contents/Resources/bin/daemon" npm run test --workspace=@ait/mobile -- src/runtime/rust-daemon/daemon.integration.test.ts
npm run typecheck --workspace=@ait/client
npm run typecheck --workspace=@ait/mobile
npm run build:desktop-assets
npm run check:docs
```

需要端口和 PTY 的定向测试在允许这些本机资源的环境中执行；默认沙箱拒绝端口绑定的首次失败不属于产品回归。真实 iOS/Android 设备和远程主机未验证，五分钟运行观察也不能证明长期绝无告警。

## Test coverage

**Not measured**：用户未请求覆盖率，按仓库规则仅执行受影响代码及直接相关行为的定向测试。测试数量与本机观察不代表行覆盖率；没有引用历史覆盖率作为本次结果。若后续请求测量，使用 `cargo llvm-cov` 对 `model` 与 `api` 的上述定向测试生成报告，记录可比基线、修复提交和实际测量范围。
