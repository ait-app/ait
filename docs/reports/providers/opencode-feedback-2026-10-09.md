# OpenCode 使用问题追踪

本会话持续追加反馈。PR 基线：`4631bb1006ce77fd36b4ee8abf4802dc7e37d7b5`；日期：2026-10-09。
以下原生权限接入的测量源码提交：`0831d51e53d4a3c623f899753158779918e845ab`。
后续 UI 修正及验证单独记录，不复用这次覆盖率。

| 编号 | 用户反馈 | 状态 |
| --- | --- | --- |
| OC-001 | Build 下拉选项缺锤子图标，选中态显示机器人 | main 的 #228 已提供 Hammer 注册、静态 manifest 和统一图标解析；本 PR 补图标回归测试，定向测试通过 |
| OC-002 | 需要像 Codex 一样接入 OpenCode 原生权限控制 | 已增加独立 Permissions 选择入口，接入原生会话 allow / ask / deny；OpenCode 1.18.4、2.0.20 隔离真实二进制测试通过，实际桌面复测待完成 |
| OC-003 | Allow / Ask / Deny 均显示同一个对勾盾牌，需要不同图标 | 已按原生值区分对勾盾牌、问号盾牌、关闭盾牌；工具栏与移动端菜单共用解析；[验证记录](opencode-permission-icons-2026-10-09.md) |

## 原生方案

这是原生权限控制的功能接入，沿用当前审批 Bridge 及 once / always / reject 行为。

- Permissions 使用现有 provider feature select 展示 Allow / Ask / Deny，Build / Plan 保持独立。
- 未选择（包括复制草稿传入 `permission: null`）时不改写原生规则；显式选择把当前原生会话的 `*` 通配权限设为所选原生 effect，
  从下一轮开始生效。它不是 OS sandbox，也不通过 Ait 自动响应 ask 来实现 allow。
- v1 使用 `PATCH /session/{id}` 的 permission / pattern / action；v2 使用
  `PATCH /api/session/{id}` 的 action / resource / effect。v1 追加单条，v2 保留原数组后替换。
- 原有规则仍保留，但新通配规则按 OpenCode 的最后匹配优先语义生效。显式 Allow 可以覆盖
  先前的会话工具限制；OpenCode 的额外原生策略仍由 OpenCode 执行。
- 不写全局或项目配置文件。选择通过 Ait 现有偏好及会话配置保存，原生规则保存在会话内。
  重复提交或恢复不重复追加同一尾规则，切换到 Ask / Deny 通过原生规则重新限制权限。
- 本入口控制会话通配规则，不提供细粒度规则编辑器或原生已保存审批列表管理。
- 写入前检查原生 idle，写入后核对完整规则；失败不提交 prompt、不自动重试不确定写入。
- 双版本细节仍封装在私有 protocol 模块，无 domain / RPC 新协议或第二套审批状态机。

原生依据：[v1 session update 源码](https://github.com/anomalyco/opencode/blob/v1.18.33/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts)、
[v2 Session API](https://opencode.ai/v2/docs/api)、[v1 权限](https://opencode.ai/docs/permissions/)、
[v2 权限](https://opencode.ai/v2/docs/permissions)。边界约定见 [ADR-074](../../decisions/providers/adr-074-opencode-native-provider.md)。

## 审查修正

- 复制草稿传入的 `permission: null` 现在按原生继承处理，不改写会话规则；双版本创建、提交、恢复均有回归测试。
- 非法 effect 返回 AgentCapabilityUnsupported；合法选择遇到原生 busy 返回 SessionBusy，避免把会话竞争误报为配置错误。
- 双版本 fixture 模拟 PATCH 成功后规则未保存，以及尾规则正确但原规则丢失。
  两种回读异常均使 start_turn 失败且不提交 prompt；修复原生规则后，同一 writer 仍拒绝后续回合。

## 验证结果

- OpenCode 定向测试：79 passed、8 ignored、0 failed。
- 普通 workspace 测试：2,051 passed、17 ignored、0 failed，包含文档测试。
- 覆盖率 workspace 测试：2,050 passed、17 ignored、0 failed；文档测试不计入覆盖率。
- 真实 OpenCode 权限测试额外执行：1.18.4 与 2.0.20 各 1 passed、0 failed。
  每个版本覆盖 allow 执行 shell 无审批、ask 交付原生审批并由测试显式回复 once、deny 不暴露 shell 工具。
- 前端图标、模式与控件布局沿用此前 14 passed 的定向结果；本次审查修正没有前端源码变化。
  workspace build、all-targets Clippy（-D warnings）、Rustfmt、文档链接和 diff 空白检查通过。
- 首次并发普通全量测试在现有 filesystem 的 10 秒排队刷新断言失败；该测试单独复测和串行全量复测通过，
  filesystem 源码未变更。完整尝试结果保存在覆盖率摘要中。
- 原生测试使用独立 XDG 配置及临时工作区，loopback 模型只运行 pwd，不使用用户模型凭据或真实推理额度，
  不替换已安装二进制。实际桌面 UI、Linux / Windows 的真实原生权限测试尚未验证。

## Test coverage

| 测量范围 | 已覆盖 / 可执行行 | 行覆盖率 |
| --- | --- | --- |
| workspace | 57,213 / 60,730 | 94.21% |
| provider crate | 27,763 / 29,757 | 93.30% |
| OpenCode 模块 | 3,414 / 3,986 | 85.65% |
| client.rs | 400 / 434 | 92.17% |
| live.rs | 338 / 382 | 88.48% |
| protocol/permissions.rs | 196 / 210 | 93.33% |

- 制品：[可审阅覆盖率摘要](opencode-permissions-pr-coverage-2026-10-09.json)，包含每文件统计、
  源码哈希、实际命令、ignored 测试清单与未覆盖行。HTML 本地生成于 `target/llvm-cov/html/index.html`。
- 源码 revision：`0831d51e53d4a3c623f899753158779918e845ab`；macOS arm64，Rust 1.98.1、cargo-llvm-cov 0.8.7，默认 features，未额外排除文件。
- 与 [PR 上一轮同范围测量](https://github.com/ait-app/ait/blob/d05911aa898930f53dbbcdc0b9246a1338addace/docs/reports/providers/opencode-permissions-pr-coverage-2026-10-09.json)（`f0927da0152af31b909608957c7f767f809909ad`）相比，
  workspace 从 94.19% 变化 +0.02 个百分点，
  provider 从 93.27% 变化 +0.03 个百分点；
  两次均采用相同默认 features、文件排除、macOS 平台及真实二进制版本。
- 全 workspace 测量后合并两次真实权限测试的 Ait profile；OpenCode 外部二进制本身未插桩。
  cargo-llvm-cov 默认排除 tests / examples / benches、测试模块文件、依赖、工具链和覆盖率构建产物；
  未启用 doctest 覆盖率、branch 覆盖率或其他平台。TypeScript 行覆盖率未测量，测试结果独立列于上方。
- workspace 默认 ignored 的 17 个测试中，仅新增的真实权限测试额外运行；其他 ignored 测试未执行。
- 非 idle / 非法 effect 和 PATCH 后回读不匹配分支已覆盖。
  重要未覆盖分支仍为 4,096 条规则上限和超限拒绝；需要补专项 fixture。
  桌面选择器及 Linux / Windows 的真实原生权限运行是后续验证项。

覆盖率生成与合并命令如下；`OPENCODE_V1_BIN` / `OPENCODE_V2_BIN` 分别指向上面记录版本的隔离测试二进制，
完整执行命令保存在摘要制品中。`--no-report` 保留先前的 profile 以合并测量。

```sh
nix develop --command cargo test -p provider local::opencode --locked --offline -- --test-threads=4
nix develop --command cargo test --workspace --locked --offline -- --test-threads=1
nix develop --command cargo build --workspace --locked --offline
nix develop --command cargo clippy --workspace --all-targets --locked --offline -- -D warnings
nix develop --command cargo fmt --all --check
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=4
AIT_TEST_OPENCODE_BIN="$OPENCODE_V1_BIN" nix develop --command cargo llvm-cov --no-report -p provider --lib --locked --offline -- installed_opencode_permission_control_uses_native_allow_ask_and_deny --ignored --nocapture
AIT_TEST_OPENCODE_BIN="$OPENCODE_V2_BIN" nix develop --command cargo llvm-cov --no-report -p provider --lib --locked --offline -- installed_opencode_permission_control_uses_native_allow_ask_and_deny --ignored --nocapture
nix develop --command cargo llvm-cov report --html
nix develop --command cargo llvm-cov report --json --summary-only --output-path target/permissions-coverage-summary.json
nix develop --command cargo llvm-cov report --lcov --output-path target/permissions-coverage.lcov
npm run check:docs
git diff --check
```
