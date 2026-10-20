# ADR-173：安全恢复客户端诊断与会话恢复

- 状态：Accepted
- 日期：2026-10-20
- 修订：ADR-161、ADR-166；取代 ADR-170 的临时回滚状态，保留其事故记录。

## 背景

#381 的诊断入口仅检查 `window` 是否存在，就调用浏览器事件方法。React Native
也有 `window`，但没有完整的 EventTarget API，因而在路由初始化前抛错。
#399 完整回滚时同时撤去了不依赖原生 UI 的 Codex 原生错误详情及重启中断轮次恢复。
后续 #402、#403 和 #404 分别修复 xterm 浏览器字段假设、提前安装 polyfill 及 iOS 场景生命周期。

## 决策

1. 恢复 #381 中有界、脱敏、仅保留在内存的诊断记录、用户主动复制／导出、Codex
   原生错误详情和 daemon 重启后中断轮次收尾。恢复仍遵守原有持久化先于状态发布、
   不自动重发输入及不记录请求正文的约束。
2. 仅在 `addEventListener` 与 `removeEventListener` 均为函数时安装浏览器监听器。
   清理使用安装时的同一个事件目标；原生环境继续链式调用 ErrorUtils。
   即使诊断采集自身失败，原来的原生错误处理器也必须运行。
3. 自动采集的 console、Query、Mutation、Provider `turn_failed`、未捕获异常和
   Promise 拒绝均使用 `notify: false`。保留设置中的诊断导出，以及已有内联失败状态。
   调用方明确发出的 `toast.error` 和操作失败对话提示继续提供反馈与日志复制入口，
   不因为自动采集再弹出一条提示。
4. 保留早期 polyfill 为 `index.ts` 的第一个导入，以及 xterm 补丁和 UIScene 修复。
   恢复旧功能不能覆盖这些后续修复；CI 必须执行原生形状全局变量下的启动测试。

## 验证与 Test coverage

回归覆盖没有 window、原生空 window、不完整事件 API、浏览器事件清理、采集器失败、
原生处理器保留，以及自动日志静默和主动操作提示。真实终端依赖还在独立 VM 中验证
React Native、Node 与浏览器启动行为。Rust 验证限定 Codex、诊断与重启恢复和相关进程测试。

**not measured**：用户未请求覆盖率。若后续需要，使用 `cargo llvm-cov` 限定上述 Rust
模块，并单独测量客户端诊断用例；测试通过数量不代表行覆盖率。Linux 本地验证不能
替代 iOS／Android 二进制在真机上的启动和键盘验收，移动端分发仍需重新构建安装包。
