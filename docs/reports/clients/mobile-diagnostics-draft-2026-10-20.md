# 客户端恢复 Draft 拆分验证

- 日期：2026-10-20；Linux；主线基线 `e3f0dac7`。
- 来源：#406 的客户端与 SDK 部分；按用户要求保持 Draft，暂缓进入主线。
- 决策：[ADR-176](../../decisions/clients/adr-176-deferred-client-diagnostics.md)。

## 待审范围

诊断启动检查浏览器监听方法能力，自动采集静默记录有界脱敏日志，保留用户主动复制／
导出和操作失败反馈。SDK 请求关联日志、共享 Web／Desktop 界面和 Toast 类型一起留在
本 Draft，以免拆断客户端依赖。

Workspace 菜单增加新建会话，模型选择行使用通用图标。终端 callback 拒绝与输出队列
失败进入日志，不自动重发输入或重放部分输出；旧任务失败不清空新快照。

沿用主线 #409 的共享 React Native 终端输入，未恢复已删除的 UIKit 模块、iOS 专用
输入文件或独立 UIKit CI job。沿用 #408 的分屏和 #413 的构建／界面修复。
只在既有 UI CI 中加入本 Draft 的相关回归。没有 Rust 变更，不依赖后端拆分 PR。
Android 编译服务及发布流程不在范围内。

## 验证

在上述基线加本报告所在提交的客户端恢复上重新运行：

- 22 个文件、115 项单测通过：诊断启动、收集与复制、静默错误、RootErrorBoundary、
  Query 错误、早期 polyfill、共享终端输入及平台路由、输出 drain、终端 callback、
  模型选择与 Workspace 菜单。
- SDK 的 `src/daemon-client.test.ts` 和 `src/diagnostic-log.test.ts` 共 177 项通过。
- `npm run typecheck --workspace=@ait/mobile --workspace=@ait/desktop` 通过。
- `npm run build:ui-deps` 的协议、SDK、highlight 和音频模块构建通过。
- `npm run check:ui` 对 98 个改动 UI／SDK 文件的严格 lint 通过；
  `node --test scripts/nightly-workflow.test.mjs` 的 26 项既有 CI 契约通过。

客户端回归命令与 `.github/workflows/ci.yml` 中的
`Check deferred client diagnostics and shared terminal regressions` 步骤一致；SDK 使用
`vitest run src/daemon-client.test.ts src/diagnostic-log.test.ts`。工作树复用本机锁定依赖，
临时文件使用独立 `TMPDIR`。没有把 #406 的旧 UIKit 测试当作当前验证结果。

## 待验收

尚未做本 Draft 的 iOS／Android 安装包构建、真机冷启动、中文组合输入和前后台切换
验收，也没有取得聊天中偶发闪退的原生 crash stack；不能据此宣称所有闪退已修复。
Draft 在完成原生验收并经维护者决定后才能转为可合入状态。

## Test coverage

**not measured**：用户未请求覆盖率，没有生成覆盖率产物。后续如需测量，仅限定上述
客户端与 SDK 用例。本分支不修改 Rust，按 AGENTS.md 不运行 Rust 测试。
