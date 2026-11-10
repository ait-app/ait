# 提问弹层响应超时与重试：验证

本次修复基于 `eatable-kangaroo` 分支的 `7e874cf6`，对应本报告所在变更。
验证平台为 macOS、Vitest 与 Playwright Chromium，使用隔离的 Rust daemon
及离线 Codex 原生协议夹具，没有使用真实账户或开发者 daemon。

## 修复行为

提问弹层的取消或提交等待最多 5 秒。超时或立即失败后，按钮恢复文字与可点击状态，
弹层保留已有选择和填写内容，用户可以再次取消或提交；等待期间继续禁用按钮。

之前，按钮记住了上次点击的操作，却没有根据请求是否仍在等待决定显示转圈，
因此请求失败后仍可能一直显示转圈。现在转圈同时受请求等待状态控制，
共享的权限操作按钮也采用相同条件。

提问响应使用整个操作的 5 秒截止时间，包括发送和响应清理阶段。
迟到的 Promise 失败不会改变后续重试的等待状态。Mutation 不因浏览器离线状态暂停，
也不自动重试，由用户再次点击发起操作。

Rust 协议适配器原先保留未完成 RPC 最长 5 分钟，复用同一个权限 ID 会被判为重复请求。
现在权限响应 RPC 在 5 秒后清理占用；该期限适用于提问及其他权限响应，
其他 RPC 与文件上传的期限保持原值。成功完成仍以服务端权限解决事件为准。

## 定向验证

```sh
npm exec --workspace=@ait/mobile -- vitest run src/agent-stream/question-response.test.ts src/components/question-form-card.test.tsx src/components/question-form-card-core.test.ts
npm exec --workspace=@ait/mobile -- vitest run src/runtime/rust-daemon/transport.test.ts -t 'releases a permission ID|expires pending RPCs|preserves permission identity|rejects a duplicate pending ID'
npm run test:e2e --workspace=@ait/mobile -- e2e/browser/question-response-timeout.spec.ts
npm run typecheck --workspace=@ait/mobile
npm run check:ui
npm run check:docs
```

- 13 项提问响应与表单单元测试通过：5 秒截止、立即失败、成功清理计时器、
  旧请求迟到失败不影响新请求、恢复按钮并保留文字答案、等待期间阻止重复取消。
- 4 项协议适配器定向测试通过，其余 37 项未运行：权限 ID 在 5 秒释放、
  同 ID 重试、权限标识保持不变、其他 RPC 期限与重复 ID 拒绝行为。
- 2 项浏览器用例通过：分别丢弃首次取消和提交请求，确认约 5 秒后恢复按钮、
  保留选项，再次点击实际送达 daemon 并关闭弹层。
- TypeScript、修改文件的 UI lint、格式和本地文档链接检查通过。
  ESLint 无错误；`view.tsx` 中已有的无关 Hook 依赖警告仍为 1 条。

没有验证安装包或原生移动设备。首次浏览器测试的拦截器使用了旧协议消息格式，
没有丢弃目标请求；改为 Rust v1 的 `method` / `params` 格式后，两个用例通过。

## Test coverage

未测量行或分支覆盖率，用户未请求覆盖率测量。上述数量仅表示功能验证结果。
本次没有修改 `bins/` 或 `crates/` 中的 Rust 代码，按仓库要求跳过 Rust 测试。
浏览器夹具构建 daemon 并启动隔离实例，没有运行 Rust 单元测试、全工作区测试或覆盖率。
