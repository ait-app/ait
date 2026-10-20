# 0.1.9–0.1.11 移动端事故回顾与恢复验证

基线：`39675e3b`（0.1.12），日期：2026-10-20。本报告区分代码可复现的故障、
已经合入的修复，以及聊天中报告但尚未取得原生崩溃日志的现象。

## 问题与处理

| 现象或要求                         | 证据与本次处理                                                                                                                                                                                                                                   |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 0.1.9 卡在启动画面                 | #381 的诊断入口假设有 window 就有浏览器事件 API；在原生形状的独立 VM 中可复现。补齐函数能力检查、原事件目标清理和 ErrorUtils 链式处理，并恢复有用的日志采集。                                                                                    |
| 自动诊断弹窗过多                   | 保留 #393 的静默 console／Query 策略，并扩展到 Mutation、Provider 终态及未捕获异常。自动采集只记录；已有操作失败提示和主动复制／导出保留。                                                                                                       |
| #381 被整体回滚                    | 恢复 Codex 原生错误详情、SDK 结构化诊断和宿主重启后中断轮次收尾；不会自动重发用户输入。保留 #399 的事故 ADR。                                                                                                                                    |
| 0.1.10／0.1.11 原生启动崩溃        | main 已合入 #402 的 xterm 补丁、#403 的早期 polyfill 和 #404 的 iOS 场景生命周期修复。本次保留这些修改，把既有启动测试加入实际 CI。                                                                                                              |
| 移动端终端输入时偶发闪退           | 确认原生输入／方向键／焦点／resize 回调和键盘 bridge 的异步拒绝存在未处理路径，解析队列的后台 Promise 拒绝也未捕获；补异常边界、日志及回归。失效输出不重放，新的快照不被旧任务失败清空。尚无真机堆栈，不能认定这些就是聊天中全部原生闪退的根因。 |
| 终端触控优化被回滚                 | #346 的键盘适配和触控分屏仍在基线，未发现第二个相关完整回滚。保留现有适配并验证其直接相关策略。                                                                                                                                                  |
| Workspace 菜单缺少新建会话         | 新增直接创建当前 Workspace 会话草稿的菜单项；保留标签切换器入口。中文入口明确为“新建会话”。                                                                                                                                                      |
| 创建模型入口沿用上次 Provider 图标 | 可切换 Provider 的模型行使用通用图标；只有具体模型已选中时才在工具条使用 Provider 图标。已有固定 Provider 会话仍可识别其来源。                                                                                                                   |
| 定时任务保存失败                   | #397 已修复“运行中任务更新”的错误码和表单提示；保留并复跑 12 项相关客户端回归。聊天未提供其他写入失败的请求或日志，不能把所有保存失败都归因于此。                                                                                                |
| Android 编译服务                   | 用户明确排除本次范围；不修改发布、签名、Runner 或构建服务。                                                                                                                                                                                      |

## 事故为什么通过了检查

最初的诊断测试在 Node 中运行，没有提供“window 存在但无 EventTarget 方法”的原生环境。
另外，CI 只选择部分客户端用例，新增的诊断测试及后来新增的 xterm 启动用例没有进入
实际执行列表。类型检查与 JS／Hermes 打包只能证明解析和依赖可用，无法证明运行时具有浏览器全局 API。

#394 的启动问题属于另一条链路：移除了布局中的 navigator 初始化，却没有同步上游提前
安装 polyfill 的完整入口。后续修复说明跨上游移植需要检查模块求值顺序，单独迁移调用点不够。
这些结论来自 PR 与代码；不能据此把之后所有崩溃合并成同一个原因。

## 本次回归门禁

- CI 的 UI job 明确执行诊断、真实依赖的原生启动、输入桥接、解析队列、模型选择及菜单回归。
- 新增独立 UIKit 键盘 job，直接运行现有组合输入 XCTest，无需完整 App、账户、daemon 或签名。
  只在原生键盘模块、iOS bridge 或 CI 配置变化时运行。
- 运行中的任务拒绝更新仍使用原有明确错误码，表单不丢失未保存输入。
- 日志收集失败不得覆盖原生致命异常；错误处理器必须继续调用系统处理器。

## 本地验证

Linux x64，系统 Rust 1.99.0。仓库与 CI 固定的版本仍为 1.98.1，本次没有调整工具链。

- Rust：Codex **189 passed / 1 ignored**、重启恢复 **5 passed**、本地诊断 **9 passed**、
  OpenCode HTTP 观测 **2 passed**；daemon OpenCode 进程回归 **5 passed**。
  未运行本地完整 Rust workspace 测试。
- 客户端：30 文件 **168 passed**，包含解析销毁与旧任务拒绝回归。
  SDK 的 `daemon-client`、`diagnostic-log` 两文件 **177 passed**。
- `npm ci`、共享 UI 依赖构建、mobile／desktop 类型检查、改动文件严格 oxlint、
  Rust 格式、文档链接检查及 workspace 构建通过。
- Provider 全目标严格 Clippy 通过。系统 Rust 1.99 的 workspace 全目标 Clippy 在未修改的
  `crates/host-link/src/controller/tests.rs:474` 被新增的 `assert_is_empty` lint 阻止；
  workspace 构建同时报告未修改 filesystem 代码中的 `fetch_update` 弃用警告。
  不修改这些无关代码，固定版本的结果由 PR CI 核验。
- CI 工作流契约 26 项通过，包含原生键盘检查失败时阻止 nightly 发布；生产三端导出与远端 CI 结果将在实际完成后记录。

Rust 定向命令：

```sh
RUSTC_WRAPPER= cargo test --locked -p provider --lib local::codex
RUSTC_WRAPPER= cargo test --locked -p provider --lib service::agent_manager::tests::recovery
RUSTC_WRAPPER= cargo test --locked -p provider --lib local::diagnostics
RUSTC_WRAPPER= cargo test --locked -p provider --lib local::opencode::observation::http
RUSTC_WRAPPER= cargo test --locked -p daemon --test process opencode_
```

## 验收限制

本机没有 Android SDK／模拟器，也没有 Xcode 或 iOS 设备。组件回归、原生形状 VM、
Hermes 产物和 UIKit CI 分别验证不同边界，不能代替完整安装包的真机测试。
发布前应确认两端冷启动、前后台切换、终端中文组合／回车／方向键／粘贴，以及
断线重连和切换终端时的输入。不在没有 crash stack 的情况下宣称原生偶发崩溃已全部消失。

## Test coverage

**not measured**：用户未请求覆盖率，以上数量只表示测试执行。没有生成或借用旧报告的
覆盖率百分比、基线或产物。若后续请求，使用 `cargo llvm-cov` 限定 Codex／重启恢复／
诊断模块，客户端限定本次启动、终端与菜单用例；完整 workspace 测量仍需明确请求。
