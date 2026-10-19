# 错误诊断与会话恢复：PR 整合验证

- 日期：2026-10-19。
- 基线：`main` 的 `50d40091`；保留原恢复提交 `1638fa31`，整合提交 `8bde482a`，
  最终结果对应本报告所在提交。
- 范围：用户要求一并收尾的 Codex 错误、客户端错误日志、宿主异常重启恢复三个 session。
- 平台：Linux x86_64、Rust/Cargo 1.99.0、Node 24.22.0；Rust 使用默认 features。
  仓库和 CI 的锁定工具链仍为 1.98.1。

## 最终行为

1. Codex 原生错误不再丢失具体原因。聊天、持久会话状态和终态事件显示一致的 HTTP／RPC
   状态、脱敏后的原生说明及已知重试结果；例如 HTTP 403 的 `insufficient balance`。
   新输入清除旧说明，重试恢复成功不显示失败，不自动重发输入。
2. 客户端错误提示保留原始异常 cause、stack、请求身份及近期日志。Toast、内联错误、
   Provider 聊天和渲染兜底均可复制／导出日志，错误提示排队并保持到手动关闭。
   桌面日志尾部异步收集，读取或复制失败不会丢掉原报告；缓存和导出前脱敏。
3. daemon 重启后，在服务请求前收尾没有 live writer 的遗留 running，显示固定重启中断原因。
   持久化失败阻止启动，重试不会重复提示；保留原生句柄和配置，显式 Continue 仍使用 ACP。

已合入最新 main，保留其旧中继清理和工作区重置行为。冲突仅在文档索引，现已合并；
客户端与 Codex 的新 ADR 分别使用 161、162，避免与 main 已有的 ADR-160 冲突。
原恢复 PR 的源码与进程测试完整保留，不覆盖另外两个 session 的实现。

决策与详细根因：
[客户端诊断 ADR-161](../../decisions/clients/adr-161-copyable-error-diagnostics.md)、
[Codex 说明 ADR-162](../../decisions/providers/adr-162-codex-error-details.md)、
[Codex 排查](../providers/codex-upstream-errors-2026-10-19.md)、
[OpenCode／宿主重启排查](../providers/opencode-stalled-turn-recovery-2026-10-19.md)。

## 验证结果

| 范围                                                   | 结果                  |
| ------------------------------------------------------ | --------------------- |
| Codex adapter                                          | 188 passed、1 ignored |
| 共享错误分类                                           | 9 passed              |
| Agent 执行、状态、目录与恢复                           | 208 passed            |
| OpenCode 离线 adapter                                  | 83 passed、27 ignored |
| daemon OpenCode 真实进程故障注入                       | 5 passed              |
| 客户端诊断、错误组件、相关 hooks 与翻译，14 个测试文件 | 103 passed            |
| SDK 请求及诊断日志，2 个测试文件                       | 177 passed            |

合计 **493 项 Rust 定向测试、280 项客户端／SDK 测试通过**。
28 项 ignored 需要真实 Codex／OpenCode 安装或模型认证，本次保持跳过。
没有在本地运行完整 Rust workspace 测试或覆盖率。

client、mobile、desktop 类型检查、UI 依赖构建及 Web 导出通过；Web 导出使用两个构建 worker。
87 个改动 UI 文件的严格 lint、Rust 格式、全仓 Markdown／TypeScript 等格式检查及文档链接检查通过。
provider 全 targets 和 daemon 生产二进制的 scoped 严格 Clippy 通过，workspace 构建通过。

本机 1.99.0 的全 workspace 严格 Clippy 在未修改的 host-link 测试触发 `assert_is_empty`；
包含依赖的 daemon lint 还触发 schedule 的 `single_element_loop` 与 filesystem 的
`fetch_update` 弃用。上述源码不在本次 PR 差异中；daemon 使用 `--no-deps` 复核自身生产目标。
workspace 普通构建保留该 filesystem 弃用警告。CI 使用仓库锁定的 1.98.1，不升级工具链。

```sh
cargo test --locked -p provider --lib local::codex --quiet
cargo test --locked -p provider --lib local::diagnostics::tests --quiet
cargo test --locked -p provider --lib service::agent_ --quiet
cargo test --locked -p provider --lib local::opencode --quiet
cargo test --locked -p daemon --test process unix::opencode --quiet
cargo clippy --locked -p provider --all-targets -- -D warnings
cargo clippy --locked -p daemon --bin daemon --no-deps -- -D warnings
cargo build --locked --workspace
# 已执行，受上述本机工具链的基线 lint 阻挡
cargo clippy --locked --workspace --all-targets -- -D warnings
npm run test --workspace=@ait/mobile -- src/diagnostics src/components/error-notifications.test.tsx src/components/root-error-boundary.test.tsx src/components/root-error-details.test.ts src/data/query-client-errors.test.ts src/assistant-file-links/use-file-link.test.tsx src/hooks/use-image-attachment-picker.test.tsx src/i18n/resources.test.ts
npm run test --workspace=@ait/client -- src/daemon-client.test.ts src/diagnostic-log.test.ts
npm run typecheck --workspace=@ait/client --workspace=@ait/mobile
npm run typecheck --workspace=@ait/desktop
npm run build:ui-deps
npm exec --workspace=@ait/mobile -- expo export --platform web --max-workers 2
npm run check:ui -- origin/main
cargo fmt --all --check
node_modules/.bin/oxfmt --check .
node --test scripts/check-docs.test.mjs
node scripts/check-docs.mjs
```

没有修改服务配置、部署、重启正在运行的 daemon 或调用真实模型。移动真机及完整打包版
Electron 本轮未验证；宿主状态恢复不防止系统再次 OOM，错误可见性也不消除上游服务故障。

## Test coverage

**not measured**：用户未请求覆盖率测量；通过数量不代表行覆盖率，没有生成覆盖率产物。
如后续请求，使用 `cargo llvm-cov` 限定 provider 的 Codex／恢复模块和 daemon OpenCode
进程测试，客户端限定本报告列出的 Vitest 文件；不默认扩展至完整 workspace 测量。
