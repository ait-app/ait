# ADR-176：暂缓恢复客户端诊断与移动交互

- 状态：Draft；按用户要求暂不进入主线。
- 日期：2026-10-20
- 来源：从 #406 拆出；后端恢复独立提交，客户端不依赖后端 PR 才能构建。
- 修订：ADR-161、ADR-166 的客户端恢复方案。

## 待审范围

1. 恢复内存有界、脱敏的客户端诊断、主动复制／导出和 SDK 请求关联日志。
   console、Query、Mutation、Provider 终态和未捕获异常自动采集均静默记录；
   显式操作失败保留原有反馈。浏览器监听检查函数能力，原生错误处理器继续运行。
2. Workspace 菜单增加新建会话；可切换 Provider 的模型选择行使用通用图标。
3. 终端回调拒绝和解析队列失败进入日志，不自动重发输入或重放部分输出。
   旧任务失败不清空新快照，销毁后的 flush 可以结束。
4. 全部沿用 main 的 #409 共享 React Native 终端输入、#408 的平台分屏支持，以及
   #413 的构建与界面修复。不恢复已删除的 UIKit 模块、iOS 专用输入文件或对应 CI job。
5. 共享 Web／Desktop 界面与 SDK 也随本 Draft 暂缓，因为它们依赖相同诊断入口和 Toast 类型。
   此分支不包含 Rust daemon／Provider 修改，不改动构建服务或发布流程。

## 验证与 Test coverage

按当前主线重新验证诊断、共享终端输入、模型和 Workspace 菜单、SDK、类型检查及 lint。
不把 #406 在旧 UIKit 实现上的测试结果作为本 Draft 的当前验证。
范围、结果和未验收的平台见[Draft 拆分报告](../../reports/clients/mobile-diagnostics-draft-2026-10-20.md)。

**not measured**：用户未请求覆盖率。后续如需测量，仅针对上述客户端与 SDK 用例；
本 Draft 不修改 Rust，因此不运行 Rust 测试。
