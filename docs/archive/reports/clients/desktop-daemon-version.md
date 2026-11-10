# 桌面 daemon 版本状态修复

源码基线：`334d3370` 加本次桌面修复；平台：macOS arm64。

## 行为

daemon 的 `server_info` 握手已有 `info.version`，桌面管理器只读取 `server_id`，导致状态中的
`version` 一直为 `null`。现在启动和重启读取实际版本；停止、启动中和失败状态清空版本，
避免把上一次运行的版本显示为当前版本。旧服务不返回字符串版本时仍使用 `null`。

协议、身份文件与环境变量未变更。

## 验证与 Test coverage

在未修复源码上，真实 daemon 生命周期回归断言失败：预期 `0.0.13`，实际 `null`。
修复后同文件 6 项全部通过，断言启动与重启版本等于包版本、停止后清空版本。
桌面完整 Vitest：53 个文件，377 项通过、9 项 Linux 专属用例在 macOS 跳过。

```sh
cd apps/desktop
AIT_SERVER_BIN="$PWD/../../target/debug/daemon" node ../../node_modules/vitest/vitest.mjs run --exclude 'e2e/**'
cd ../..
npm run typecheck --workspace=@ait/desktop
npm run build:main --workspace=@ait/desktop
```

TypeScript 检查、主进程构建、Oxfmt、Oxlint 均通过。未启用 TS 覆盖率收集，不能据测试通过数
推导行覆盖率。本次不修改 Rust；按仓库规定不重复运行 Rust 测试，Rust 覆盖率见
[重命名验证报告](../daemon/workspace-rename.md)。
