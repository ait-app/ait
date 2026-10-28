# MiniMax Code ACP 接入验证

- 源码基线：`bca1f98b`，加本次工作区改动。
- 平台：macOS arm64；Rust 1.98.1、Node.js 26.12.1。
- 原生程序：`@minimax-ai/code` **0.7.1**，2026-10-28 从 npm latest 查询并安装。
- 接入参考：Paseo 的 MiniMax Code ACP 目录及通用 ACP adapter。运行程序使用本机
  最新 `mcode acp`，不使用 Paseo 目录中的历史固定版本。

## 测试范围

离线协议夹具覆盖模型/模式发现和切换、原生上下文恢复、文本与推理投影、允许/拒绝
审批、重复审批拒绝、停止确认/超时、会话身份校验、认证错误和错误脱敏。
执行器测试通过 AIT 的 catalog/create/send/wait/timeline/resume 接口验证持久化协作。

真实程序测试使用用户完成的原生登录，在隔离临时工作目录执行；不复制认证文件或
保存模型服务凭据。三项真实测试验证了从 AIT 发消息、取得回答、关闭执行器、恢复
同一原生 session 并正确回忆测试标记，以及工具写文件、取消和模型/控制项选择。

真实工具测试收到 2 条工具事件并检查了文件内容；原生程序对该工作区写入未发出
权限回调（0 次）。允许/拒绝/重复审批由离线协议测试覆盖，不声称真实审批分支已验证。
配置测试确认 M3.1-Flash thinking 可设置 `low`，M3 thinking 不暴露思考等级；
离线回归验证切换模型后拒绝已失效的等级选项。

## 验证命令与结果

| 验证              | 命令                                                                                                                                                               | 结果                           |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------ |
| 全 Rust 工作区    | `TMPDIR=/private/tmp cargo nextest run --locked --workspace`                                                                                                       | 2959 通过，55 跳过             |
| 文档测试          | `TMPDIR=/private/tmp cargo test --locked --workspace --doc`                                                                                                        | 通过；14 个目标均无 doctest    |
| 最终 MiniMax 回归 | `TMPDIR=/private/tmp cargo test --locked -p provider --lib local::minimax`                                                                                         | 10 通过，3 个真实测试默认忽略  |
| 真实 mcode        | `TMPDIR=/private/tmp AIT_TEST_MINIMAX_CODE_BIN=/opt/homebrew/bin/mcode cargo test --locked -p provider --lib local::minimax::tests::live -- --ignored --nocapture` | 3 通过                         |
| SDK               | `npm run test:sdk`                                                                                                                                                 | protocol 813、client 440，通过 |
| 前端 Provider     | 下方命令                                                                                                                                                           | 9 文件、144 通过               |
| 前端类型          | `npm run typecheck --workspace=@ait/mobile`                                                                                                                        | 通过                           |
| UI 依赖           | `npm run build:ui-deps`                                                                                                                                            | 通过                           |
| Web 构建          | `npm exec --workspace=@ait/mobile -- cross-env AIT_WEB_PLATFORM=electron expo export --platform web`                                                               | 通过                           |
| Rust 构建         | `cargo build --locked --workspace`                                                                                                                                 | 通过                           |

`cargo clippy --locked --workspace --all-targets -- -D warnings` 通过，无警告。
`cargo fmt --all --check`、变更文件的 `oxfmt --check`、`oxlint` 和 `git diff --check`
均通过；`npm run check:docs` 校验 472 个 Markdown 文档的本地链接通过。
浏览器端到端 2 项通过：内置入口、真实问答、刷新后续聊。执行命令：

```sh
TMPDIR=/private/tmp E2E_AIT_SERVER_BIN=target/debug/daemon \
  AIT_TEST_MINIMAX_CODE_BIN=/opt/homebrew/bin/mcode \
  npm exec --workspace=@ait/mobile -- playwright test \
  --project=browser --project=real-provider \
  e2e/browser/acp-provider-catalog.spec.ts e2e/browser/minimax-code.real.spec.ts
```

浏览器验证使用隔离的 daemon 数据目录、临时工作区和随机端口；没有替换或重启
用户正在运行的 AIT 应用。要在日常应用使用本实现，需要运行包含本次改动的 daemon。

前端 Provider 回归命令：

```sh
npm run test --workspace=@ait/mobile -- \
  src/hooks/use-acp-provider-catalog.test.ts src/provider-selection/ \
  src/utils/provider-definitions.test.ts src/data/providers-snapshot.test.ts \
  src/hooks/use-providers-snapshot.test.ts src/stores/provider-settings-store.test.ts \
  src/screens/settings/providers-section.test.tsx
```

全套首次执行发现 3 处 daemon 快照仍使用旧 Provider 数量，已同步修正；另外 15 个既有
OMP/Pi/OpenCode 用例受 macOS `/var` 与 `/private/var` 临时路径别名影响。使用绝对规范
临时根 `/private/tmp` 后完整套件通过，未降低并发。全套之后补充了 1 个默认忽略的
原生控制测试，并加强模型等级回归；最终通过上述聚焦回归和 3 项显式真实测试验证。

本机执行日志在 `/private/tmp/ait-minimax-*.log`；它们不提交到仓库。

## Test coverage

未测量。用户要求测试流程，未要求覆盖率；测试通过数不作为覆盖率。
若后续要求测量，使用 `cargo llvm-cov` 限定 MiniMax adapter、共享 ACP 投影和相关
执行器场景。本次无可比覆盖率基线，无覆盖率报告产物。

平台限制：本次真实原生程序验证只覆盖 macOS arm64；其他平台不以此报告声明已验证。
尚未实现的外部会话导入、回退、实时语音和辅助摘要见
[操作手册](../../operations/minimax-code.md)。
