# MCP 模型选择与定时任务更新验证

日期：2026-11-13。环境：Linux x86_64、Rust 1.98.1、Node.js 26.10.0。

验证源码：基准提交 `01153d65dffac77a3030766a0b4981cbe16ca2af` 加本次工作区变更。
以下命令生成的源码补丁 SHA-256 为
`abd1a3bd7050c0f53af9c6045ec86b48491458e91918617613bb0d7f21257109`：

```sh
git diff HEAD -- crates/api crates/schedule packages/protocol apps/mobile skills/ait-orchestration | sha256sum
```

## 验证

### Rust

受影响服务及其消费者共 14 个 crate：**2,850 个测试通过，0 失败，76 个忽略**。
同次命令包含这些库的 doctest 检查，均通过；当前没有可执行 doctest。

| 测试目标                         | 通过 | 失败 | 忽略 |
| -------------------------------- | ---: | ---: | ---: |
| api：单元与 fragmentation        |  301 |    0 |    0 |
| bot                              |  283 |    0 |    0 |
| daemon：单元、依赖守卫与进程集成 |  321 |    0 |    0 |
| provider：运行时与 catalog port  |  596 |    0 |    1 |
| provider-antigravity             |   72 |    0 |    2 |
| provider-builtin                 |    6 |    0 |    0 |
| provider-claude                  |   85 |    0 |    2 |
| provider-codex                   |  270 |    0 |    4 |
| provider-deepseek-harness        |  107 |    0 |    8 |
| provider-minimax                 |   19 |    0 |    4 |
| provider-omp                     |  262 |    0 |   21 |
| provider-opencode                |  127 |    0 |   30 |
| provider-pi                      |  303 |    0 |    4 |
| schedule                         |   98 |    0 |    0 |

```sh
cargo test --locked -p api -p bot -p daemon -p provider -p provider-antigravity -p provider-builtin -p provider-claude -p provider-codex -p provider-deepseek-harness -p provider-minimax -p provider-omp -p provider-opencode -p provider-pi -p schedule --no-fail-fast
```

### 协议与客户端

**3,027 个测试通过，0 失败，11 个跳过**。

| 测试目标                               | 通过 | 失败 | 跳过 |
| -------------------------------------- | ---: | ---: | ---: |
| @ait/protocol                          |  874 |    0 |    0 |
| @ait/client                            |  571 |    0 |    1 |
| @ait/highlight                         |  103 |    0 |    0 |
| @ait/mobile：CI 回归与工具详情相关测试 |  905 |    0 |    0 |
| @ait/desktop                           |  574 |    0 |   10 |

```sh
npm run test --workspace=@ait/protocol --workspace=@ait/client --workspace=@ait/highlight
npm run test --workspace=@ait/mobile -- src/components/question-form-card-core.test.ts src/runtime/host-runtime.test.ts src/i18n/resources.test.ts src/runtime/daemon-transport maestro/support native-release-version.test.ts src/timeline src/types/stream.test.ts src/runtime/replica-cache src/runtime/directory-sync src/screens/workspace/workspace-route-state.test.ts src/runtime/online-service-host-sync.test.ts src/screens/settings/online-service-host-section.test.tsx src/utils/tool-call-parsers.test.ts src/utils/diff-highlight.test.ts src/voice/audio-engine.native.test.ts src/voice/audio-engine.web.test.ts src/voice/voice-runtime.test.ts src/utils/tool-call-detail-state.test.ts src/components/permission-request-card.test.tsx
npm test --workspace=@ait/desktop
```

### 构建与静态检查

| 命令                                                                                                                                                                                                                                                                                       | 结果                                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------ |
| `cargo build --locked --workspace`                                                                                                                                                                                                                                                         | 通过                                             |
| `cargo fmt --all --check`                                                                                                                                                                                                                                                                  | 通过                                             |
| `cargo clippy --locked -p api -p bot -p daemon -p provider -p provider-antigravity -p provider-builtin -p provider-claude -p provider-codex -p provider-deepseek-harness -p provider-minimax -p provider-omp -p provider-opencode -p provider-pi -p schedule --all-targets -- -D warnings` | 通过，0 告警                                     |
| `cargo clippy --locked -p provider -p provider-codex --lib -- -D warnings`                                                                                                                                                                                                                 | 通过，0 告警                                     |
| `python3 scripts/check-paseo-client-methods.py`                                                                                                                                                                                                                                            | 187 个客户端方法与 247 个组件声明核对通过        |
| `python3 scripts/rust_method_specs_test.py`                                                                                                                                                                                                                                                | 3 个通过                                         |
| `python3 scripts/check-crate-coverage.test.py`                                                                                                                                                                                                                                             | 2 个通过                                         |
| `npm run build:desktop-main`                                                                                                                                                                                                                                                               | 协议、SDK、highlight、音频包与桌面主进程构建通过 |
| `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`                                                                                                                                                                                                                       | 两个客户端均通过                                 |
| `node_modules/.bin/oxlint --deny-warnings --ignore-pattern '.ua/**' --ignore-pattern '.memsearch/**' .`                                                                                                                                                                                    | 通过，0 告警、0 错误                             |
| `npm run check:ui`                                                                                                                                                                                                                                                                         | 7 个变更文件通过                                 |
| `node_modules/.bin/oxfmt --check . '!.ua/**' '!.memsearch/**'`                                                                                                                                                                                                                             | 通过                                             |
| `npm run check:docs`                                                                                                                                                                                                                                                                       | 文档检查器测试与本地链接、元数据检查通过         |
| `git diff --check`、`git diff --cached --check`                                                                                                                                                                                                                                            | 通过                                             |

### 附加回归

| 命令                                    | 结果                                                           |
| --------------------------------------- | -------------------------------------------------------------- |
| `node --test scripts/check-ui.test.mjs` | 3 个通过                                                       |
| `node --test scripts/verify.test.mjs`   | 1 个通过                                                       |
| `npm run verify:local-packages`         | 6 个私有 Workspace 及本地依赖链接通过                          |
| `npm run verify:release`                | 版本与锁文件一致性检查通过                                     |
| `npm run test:release`                  | 71 个通过                                                      |
| `npm run test:mobile-release`           | 2026-11-14 在基准 `921183cf` 复验：62 个通过，0 失败，1 个跳过 |

## 测试覆盖

未测量：本次未要求覆盖率统计。如需测量，可对 `api`、`schedule` 及相关服务测试运行聚焦的
`cargo llvm-cov`，并提供测量范围、行数、百分比和可共享报告。

## 基于 921183cf 的文档验证

源码基准：`921183cf` 加当前工作区变更。

| 命令                                                                                                                                        | 结果                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `npm run check:docs`                                                                                                                        | 1 个测试通过，579 篇文档链接与 239 个 ADR 元数据检查通过 |
| `node_modules/.bin/oxfmt --check docs/architecture/README.md docs/decisions/README.md docs/reports/mcp-agent-model-selection-2026-11-13.md` | 通过                                                     |
| `git diff --check`、`git diff --cached --check`                                                                                             | 通过                                                     |

## Ruby 与移动发布复验（2026-11-14）

源码基准：`921183cf78d110d879d50c32c84ea0807d22ea93` 加当前工作区变更。
环境：Linux x86_64、Node.js 26.10.0、Ruby 3.4.10。

| 命令                                                                      | 结果                        |
| ------------------------------------------------------------------------- | --------------------------- |
| `node --test apps/mobile/plugins/with-ios-pod-deployment-target.test.cjs` | 2 个通过，0 失败，0 跳过    |
| `npm run test:mobile-release`                                             | 62 个通过，0 失败，1 个跳过 |

## 移动 CI 测试清单验证（2026-11-14）

源码基准：`921183cf78d110d879d50c32c84ea0807d22ea93` 加当前工作区变更。

| 命令                                                                                                                                                                                          | 结果                            |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| `npm run test --workspace=@ait/mobile -- --project unit src/utils/tool-call-detail-state.test.ts src/components/permission-request-card.test.tsx src/composer/task-list/presentation.test.ts` | 3 个文件、23 个测试通过，0 失败 |
| `node --test scripts/verify.test.mjs`                                                                                                                                                         | 25 个通过，0 失败               |
| `planVerification` 的 `owners`、`closure`、`all` 选择校验                                                                                                                                     | 三种范围均包含上述三个测试文件  |
| `node_modules/.bin/oxlint --deny-warnings scripts/verify.mjs`                                                                                                                                 | 通过，0 告警、0 错误            |
| `node_modules/.bin/oxfmt --check scripts/verify.mjs docs/reports/mcp-agent-model-selection-2026-11-13.md`                                                                                     | 通过                            |
