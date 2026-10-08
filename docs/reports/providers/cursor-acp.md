# Cursor ACP 接入验证

- 日期：2026-10-08。
- 早期专项源码：基于 `ced7642ede7b3cc9a1ee58dad6d0f4516a1f364d` 的未提交工作区。
- PR 源码与提交前检查：见下方 Test coverage 和提交前验证记录。
- 环境：Linux；离线 Python/Node ACP 夹具，无 Cursor token 或在线模型请求。
- 范围：Cursor Provider、内部共享 ACP、Provider 装配、daemon 与客户端接入。
  PR 从最新 `main` 的 `4794dbc493aee9926f37438f8f2d5f9085e3d835` 隔离构建。
  原工作区的 DSH 插件预设属于独立 PR #229，之前的本地修复属于 PR #228；本 PR 不包含它们。
  早期专项验证在原工作区运行，下面分别记录该结果与本 PR 的提交前完整检查。

## 实现与验证范围

Cursor 支持只读模型目录、逐模型思考/Fast 控件、configOptions 与旧 models/modes 混合协议、
异步原生命令、流式文本/推理/工具状态、ACP 计划任务、上下文与 token 用量、工具审批、选择题和计划审批。
测试验证认证调用、会话环境不进入持久化、session/load 恢复、取消、图片能力协商、
foreign session 拒绝、异常帧/超限帧/认证失败/控制超时和不可用程序。

真实 daemon 离线集成验证客户端 RPC 审批流程、时间线持久化、重启后读取、恢复并取消。
共享 ACP 的标准审批及 timeline 单元测试移到新模块，原有 DSH 原生与兼容模式测试通过。
客户端选择列表复用已有 Cursor 图标，Provider override 可启用 Cursor，终端提供原生 resume 命令。

## 参考与差异复核

本轮直接读取 Paseo 上游 `23d7a089e228862478972895279d4caeb6c591db`（2026-10-08）源码，
并对照 Ait 当前 Codex、Claude Code 实现。仓库 `paseo/README.md` 原记录的较早版本并未替换。
首版主要参考官方 Cursor 文档及 Ait 的 DSH ACP；本轮补上直接的上游与跨 Provider 对照。

| 对照项 | 源码证据 | 首版缺口与本轮处理 |
| --- | --- | --- |
| Cursor 模型发现 | [Paseo Cursor ACP](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/cursor-acp-agent.ts)、[catalog 测试](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/cursor-acp-catalog.test.ts) | 改用只读 `cursor/list_available_models`，保留 ACP ID，完整目录不依赖 session/new 的当前模型列表；每个模型独立解析思考参数，空目录保持为空，扩展错误不掩盖 |
| 配置与控件 | [Paseo Cursor 测试](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/cursor-acp-agent.test.ts)、[Ait Codex controls](../../../crates/provider/src/local/codex/controls.rs)、[Claude config](../../../crates/provider/src/local/claude/config.rs) | 发现/草稿/校验禁止调用 setter；宣告参数化模型能力；补 Fast，使用 Ait 统一 `fast_mode` ID；混合 configOptions/legacy selectors 合并，先处理 setter 响应前的异步配置通知，防止旧通知覆盖新选择 |
| Fast 不支持的模型 | 同上 | 与 Ait Codex/Claude 的显式启用校验一致：Fast=true 不支持时拒绝，false 不强求原生选项；Paseo 可对先前存储的 true 降级，本次不默默忽略显式启用 |
| 原生命令 | [Paseo Cursor ACP](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/cursor-acp-agent.ts)、[Ait Codex commands](../../../crates/provider/src/local/codex/controls.rs) | 接入 `available_commands_update`，命令查询最多等十秒；正常开会话不等待命令列表 |
| 用量 | [Paseo ACP](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/acp-agent.ts)、[Paseo Claude](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/claude/agent.ts)、[Ait usage](../../../crates/provider/src/local/usage.rs) | prompt 返回 input/output/cachedRead tokens 与最新原生 context 合并为完整快照，重复计数不累加；不按 token 累计推算上下文或成本 |
| 取消与终止 | [Paseo ACP](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/acp-agent.ts)、[Paseo Codex](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/codex-app-server-agent.ts)、[Ait Claude session](../../../crates/provider/src/local/claude/session.rs) | 主动取消答复未决 callback 为 cancelled；终止时收尾未完成工具快照并保留输入；仍有未决工具/审批的 end_turn 不误报成功 |
| 恢复与时间线 | [Paseo ACP](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/acp-agent.ts)、[Ait Codex](../../../crates/provider/src/local/codex.rs) | 校验恢复返回 ID；load 优先，协商的 resume 为后备；初始化阶段历史通知不进入下一轮次；标准 ACP plan 展示任务列表 |

Cursor 专用问答和计划审批的 shape/取消 outcome 另以
[官方 ACP 文档](https://cursor.com/docs/cli/acp)核对。没有为了统一外观将 Cursor 的 Agent 模式
等同于 Codex/Claude 的 full-access/bypass 权限，或复用其他 Provider 的静态模型名单。

## 早期专项测试执行

原工作区更新后的专项 Rust 测试共 **93 通过，0 失败，5 忽略**；忽略项为现有 DSH 安装/在线环境测试。
该早期阶段没有执行完整工作区测试；提交前结果另见下文。

```sh
cargo test -p provider local::cursor --lib
cargo test -p provider local::acp --lib
cargo test -p provider local::deepseek_harness --lib
cargo test -p provider composition::tests --lib
cargo test -p daemon --test process unix::cursor::
cargo test -p daemon --test process unix::daemon::
cargo test -p daemon --test process unix::deepseek_harness::
```

对应通过数为 19、9、56、4、1、3、1。首轮 DSH 回归中的既有
`creates_and_restores_plugin_composition_without_overwriting_permissions` 在 create 时曾返回
`Unavailable`；单独重跑通过，随后整个 DSH 专项组重跑通过。未修改该原生预设路径。

本轮新增验证包含：发现与草稿无 setter、partial/hybrid 模型列表、模型参数不串用、
参数化 ID 保留、空目录/扩展失败、混合配置异步通知顺序、Fast、命令等待上限、prompt token
快照、取消后的工具收尾/审批答复、恢复 ID 不匹配与历史隔离。

首版客户端专项 **10 通过，0 失败**；本轮未改客户端源码：

```sh
npm exec --workspace=@ait/protocol -- vitest run src/provider-manifest.cursor.test.ts src/provider-manifest.antigravity.test.ts src/provider-manifest.deepseek-harness.test.ts
npm exec --workspace=@ait/mobile -- vitest run --project unit src/utils/provider-command-templates.test.ts src/components/provider-icons.test.ts
```

格式、lint、类型与文档检查：

```sh
cargo fmt --all --check
cargo clippy -p provider -p daemon --all-targets -- -D warnings
node_modules/.bin/tsgo --noEmit -p packages/protocol/tsconfig.json
node_modules/.bin/oxlint packages/protocol/src/provider-config.ts packages/protocol/src/provider-manifest.ts packages/protocol/src/provider-manifest.cursor.test.ts apps/mobile/src/utils/provider-command-templates.ts apps/mobile/src/utils/provider-command-templates.test.ts
node_modules/.bin/oxfmt --check packages/protocol/src/provider-config.ts packages/protocol/src/provider-manifest.ts packages/protocol/src/provider-manifest.cursor.test.ts apps/mobile/src/utils/provider-command-templates.ts apps/mobile/src/utils/provider-command-templates.test.ts
npm run check:docs
git diff --check
```

## 提交前完整验证

在隔离 PR 工作区运行全部检查，未修改 Rust 源码以处理环境重试。
完整 Rust 测试 **2022 通过、0 失败、15 忽略**，含 doc tests；覆盖率执行
**2021 通过、0 失败、15 忽略**，不含 doc tests 插桩。忽略项逐条记录在覆盖率 artifact，
主要需要已安装 CLI、认证或指定原生会话。客户端专项 **10 通过、0 失败**。

```sh
XDG_CONFIG_HOME=/tmp/ait-cursor-pr-test-config CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target cargo test --workspace --locked --offline -- --test-threads=1
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target cargo build --workspace --locked --offline
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all --check
npm run typecheck --workspace=@ait/protocol
npm run check:docs
git diff --cached --check
```

上述全部通过；客户端 test、oxlint、oxfmt 命令与早期专项章节相同，已在隔离源码重跑通过。
协议类型检查通过正式 npm pretypecheck 生成忽略的 validators 后运行；初次直接 tsgo 缺少
这些生成文件，因此该次失败不记为成功检查。

初次完整测试及覆盖率受到本机全局 Git `core.hooksPath=.githooks` 影响，已有
reset_workspace 夹具测试失败。成功运行使用空的临时 `XDG_CONFIG_HOME`，未更改 HOME
或用户 Git 配置。并行重试还出现既有 GitHub CLI 夹具失败，单测通过后最终完整执行采用
`--test-threads=1`；本 PR 未修改这两处已有实现。

## Test coverage

| 测量范围 | 行覆盖率 | covered / total |
| --- | ---: | ---: |
| Rust workspace | 94.13% | 57071 / 60632 |
| provider crate | 93.17% | 28046 / 30102 |
| daemon crate | 95.87% | 881 / 919 |
| Cursor 生产源码 | 92.35% | 1098 / 1189 |
| 共享 ACP 生产源码 | 94.84% | 533 / 562 |

测量源码：[代码提交 `05875ae714dfb588b7c3b1ddf75c1e6bd53c1ab2`](https://github.com/ait-app/ait/commit/05875ae714dfb588b7c3b1ddf75c1e6bd53c1ab2)，
base 为 `4794dbc493aee9926f37438f8f2d5f9085e3d835`。测量在提交前执行，
artifact 中的 `changed_source_sha256` 已逐个核对提交 blob 与实际检查的工作区源码一致；
随后验证提交仅修改文档，无 Rust/客户端源码变化。
范围为默认 features 的完整 Rust workspace，Linux x86_64；rustc/cargo 1.98.1、
cargo-llvm-cov 0.9.1、LLVM 23.1.1。采用工具默认文件过滤，没有额外 exclusions；
15 个默认忽略测试未执行，macOS/Windows 未验证，doc tests 没有插桩。
没有测量可比较的 base 覆盖率，不报告覆盖率增减。

```sh
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata XDG_CONFIG_HOME=/tmp/ait-cursor-pr-test-config CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target cargo llvm-cov --workspace --html --locked --offline -- --test-threads=1
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata XDG_CONFIG_HOME=/tmp/ait-cursor-pr-test-config CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target cargo llvm-cov report --json --output-path /tmp/ait-cursor-pr-coverage-raw.json --locked --offline
```

可审阅 artifact：[已提交的覆盖率摘要与源码 SHA-256](cursor-acp-pr-coverage-2026-10-08.json)，
包含全部 crate、变更源码的行覆盖率及独立测试执行记录。完整 HTML 已生成并审阅，
本地路径为 `/home/lonnet/Developers/ait/target/llvm-cov/html/index.html`；
该本地 HTML 路径本身不是共享 artifact，runtime profiles 与原始导出未提交。

未覆盖的重要行为包括真实 Cursor 登录、目录/模型/图片调用、可选 `session/close`
能力分支、部分溢出/非法消息保护及少见模式/配置/通知错误；行覆盖率也不能替代分支覆盖率。
后续需以已认证原生 CLI 及 macOS/Windows 安装验证，并以离线夹具补可选关闭和异常协议分支。

## 剩余差异与验证限制

与 Paseo 的通用 ACP 能力相比，本次未接入原生会话列表/导入、只读历史重放、会话级 MCP、
rewind、自动接受审批、辅助生成及 Cursor 账户额度插件。用户/项目 MCP 仍由 Cursor CLI 管理。
标准 ACP plan 已展示，但 Cursor `update_todos` 的 merge 语义、`task`/`generate_image` 扩展
专属展示尚未实现。prompt tokens/context 用量与账户订阅额度属于不同数据源。
这些是明确的剩余范围，不能称为完整 Paseo 功能对齐。

本机未安装 Cursor CLI，未执行真实 Cursor 登录、在线模型调用、真实模型目录、真实图片输出
或 macOS/Windows 安装验证。支持边界以 [操作手册](../../operations/cursor.md)与
[ADR-110](../../decisions/providers/adr-110-cursor-acp-provider.md)为准。
