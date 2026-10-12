# 定时任务列表 Host 展示去重：验证

本次修复基于 `main` 的 `aaff6d48`，对应本报告所在提交。
全局 Schedules 列表原先只在单一已配置主机场景省略行内主机名；
存在多个 Host 时，即使筛选到某个 Host，任务行仍重复显示筛选器已说明的主机名。

修复后，单一已配置 Host 或指定 Host 筛选都会省略行内主机名；
多主机的“全部 Host”视图仍显示归属。任务的 Workspace、执行目标、状态和时间信息保持可见。
使用方式见 [Workspace 定时任务](../../operations/schedules.md)。

## 定向验证

在 macOS、Playwright Chromium 上运行一项直接相关的浏览器用例，使用两个隔离的本地 daemon
和离线 Codex 原生协议夹具。用例创建跨主机 Worktree 定时任务，然后确认：

1. 全部 Host 视图中的任务行显示所属主机名。
2. 切换到指定 Host 后，任务仍可见，行内不再重复主机名，Workspace 名仍可见。
3. 切回全部 Host 后，行内主机名恢复。

```sh
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/schedules-project-target.spec.ts --grep 'global creation resets its placement'
npm run typecheck --workspace=@ait/mobile
npm run check:ui
npm run check:docs
```

浏览器用例 1 项通过，移动端 TypeScript、修改文件的 ESLint、UI lint、格式和文档链接检查通过。
本次未验证原生移动设备或安装包。

## Test coverage

未测量行或分支覆盖率，用户未请求覆盖率测量。测试通过数量是功能验证结果。
本次只修改客户端展示条件和直接相关的浏览器用例，没有修改 `bins/` 或 `crates/` 中的
Rust 代码，按仓库要求跳过 Rust 测试。浏览器夹具构建最新 main 的 daemon 并启动隔离实例，
没有运行 Rust 单元测试、全工作区测试或覆盖率。
