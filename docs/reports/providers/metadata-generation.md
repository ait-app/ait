# Server metadata generation

日期：2026-09-27。实现决策：[ADR-058](../../decisions/providers/adr-058-daemon-metadata-generation.md)。

## 结果

`metadataGeneration` 从单纯保存配置变成真实调用链，覆盖固定版本 Paseo 的四类功能：

| 入口                                                           | 生成内容                            | 保留与失败行为                                                                        |
| -------------------------------------------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------- |
| 新建/导入 session 的临时标题                                   | 首条用户内容生成标题                | 先显示文本回退；显式/原生标题保留；失败保留临时标题                                   |
| directory/worktree 的 firstAgentContext，或稍后首条 Agent 输入 | 工作区标题、独立生成的分支名        | 手工标题保留；只有 managed 随机占位分支可改名；显式、已切换、已有 upstream 的分支保留 |
| `checkout.commit.request` 省略或空白 message                   | uncommitted diff 的提交主题         | 显式内容保留；模型不可用时 `Update files`                                             |
| `checkout.pr.create.request` 缺 title 或 body                  | 指定 baseRef 的提交差异生成 PR 文案 | 仅填缺失字段；失败回退到默认标题/正文；Git 读取错误不会被吞掉                         |

配置候选按顺序优先，再试已发现的小模型，最后试当前 session 选择；禁用 Provider 不参与。
当前可执行适配器为 Codex、Claude，未新增其他 Provider。配置 set/reload 对之后的生成生效。
项目根 `paseo.json` 的 `metadataGeneration.title/branchName/commitMessage/pullRequest.instructions`
分别覆盖默认风格，固定输出 schema 和源材料约束保持生效。

共享生成器最多接纳 32 项、并发 2 项；每项总期限 90 秒，每次 Provider 调用 25 秒，
无效 JSON 最多修复两次再换候选。生成不占用前台文件系统 job permit，不创建公开 Agent，
不修改 Timeline 或前台状态。工作区结果以原子 compare-before-write 更新，保留同时修改的
labels/pin 等其他字段；手工改成与临时标题相同的文本也能阻止迟到覆盖。

新持久字段可选，旧非空标题不会被推断为可覆盖。只有记录了临时来源的标题会自动生成；
无内容时没有可生成的标题。生成失败后 session 在本次进程内保留临时标题，重启可重试；
满队列是有界回退，不阻塞用户操作。附件作为上下文源材料使用，不自动打开附件文件。
Paseo 的四类配置没有单独的 session 摘要接口，本次没有另造摘要协议。

## 对照与协议依据

参照 [Paseo 固定源码](https://github.com/getpaseo/paseo/tree/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src)：
`structured-generation-providers.ts`、`agent-response-loop.ts`、`worktree-branch-name-generator.ts`、
`workspace-auto-name.ts`、`paseo-worktree-service.ts`、`checkout/git-metadata-generator.ts`、
`build-metadata-prompt.ts`。上游不受限的异步调用替换为 Rust 有界任务与可取消原生进程。

Codex 的 ephemeral/outputSchema 由本机 app-server JSON schema 核对；功能开关依据
[官方配置参考](https://developers.openai.com/codex/config-reference/)。Claude 参数由本机
`claude --help` 核对。两种协议均有离线进程测试，没有发送真实付费模型请求。

## 验证

新增测试包含候选优先级/去重/禁用项、项目四类风格、Unicode 与 JSON 结构、无效输出重试、
期限与队列、前台许可释放、Git 错误与模型错误区分、PR 部分字段保留、原生进程临时会话与
工具限制、手工同值改名、归档、停止任务、分支所有权/upstream/切换/冲突后缀。
真实 Server + WebSocket + 离线 Provider 验证配置热更新、工作区标题、session 标题、
空提交信息、managed 分支自动命名和显式分支保留；Git 使用临时本地仓库。

最终 `cargo test --workspace`：**1752 passed，0 failed，8 ignored**（包含 1 项 doctest）。
`cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo fmt --all --check` 和 `git diff --check` 全部通过。以上 Cargo 命令均使用
`CARGO_TARGET_DIR=/private/tmp/ait-agent-interface-target`，features 为默认；基线为
`b939123f33cedb5dde06b5cad2d657cfe4436bea` 加本次工作树修改。随后 rebase 到 `0d42467453c60c1390bd7385dc4799418d941800`；
该次 main 变动只涉及 UI、CI 和文档，Rust 源码、Cargo 配置及测试替身与测量时完全一致。

8 项既有忽略测试涉及可选的真实 Provider/OS 环境；本次没有把它们改为跳过。
端口相关测试在沙箱外执行，未访问远端 Forge 或真实模型。工作区标题、随机分支和
显式分支路径均通过真实 Server 接口验证。

本地 `127.0.0.1:4567` 已使用最终代码重启，保留原数据目录和 token；启动前确认没有活动
对话，`readyz` 和带认证的 server info 均为 200。未做桌面 UI 手工验收。

## Test coverage

本次 workspace 行覆盖率为 **85.52%（58,922 / 68,902）**，相对
[初版标题修复基线](server-session-titles-coverage.json) 的 85.30% 增加 **0.22 个百分点**。
基线与当前测量均为 macOS aarch64、默认 features、全 workspace；当前代码也包含 main
后来合入的 Codex command-actions 修复，增量不全部归因于本次功能。

| 测量范围   | 已覆盖 / 总行数 | 行覆盖率 |
| ---------- | --------------- | -------- |
| Workspace  | 58,922 / 68,902 | 85.52%   |
| provider   | 15,251 / 16,415 | 92.91%   |
| metadata   | 6,061 / 6,758   | 89.69%   |
| filesystem | 7,759 / 8,821   | 87.96%   |
| api        | 1,104 / 1,149   | 96.08%   |
| daemon     | 834 / 883       | 94.45%   |

测量快照：`b939123f33cedb5dde06b5cad2d657cfe4436bea` 加本 PR 的功能修改；
[可审阅 JSON 制品](metadata-generation-coverage.json) 记录逐文件 SHA-256、行数、
完整 crate 汇总和基线，可与提交内容逐项核对。HTML 位于
`/private/tmp/ait-session-titles-coverage/llvm-cov/html/index.html`；共享制品以仓库中的 JSON 为准。

精确命令：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov --workspace --html -- --test-threads=4
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-metadata-workspace-coverage.json
```

覆盖率运行 **1751 passed，0 failed，8 ignored**；普通运行的额外 1 项为 doctest。
无显式文件排除；测试源码由 cargo-llvm-cov 默认规则排除，doctest 不计入默认插桩测量。
默认并发下 4 个已有 native transport 测试出现失败，普通构建同一批测试通过；改用
4 线程后完整测量和这 4 项测试全部通过，没有放宽生产超时或跳过测试。

主要未覆盖分支为部分原生异常帧、registry/文件 I/O 故障及队列取消竞争；这些路径保留
临时标题或返回安全错误。真实付费模型响应质量、远端网络错误和 Windows 原生进程清理
需要对应环境验收；Linux/Windows 未在本机测量。离线协议替身不等于真实模型端到端验证。
