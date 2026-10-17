# Ait 0.1.6 发布说明与验证

日期：2026-10-17（Asia/Shanghai）。上一正式版 `v0.1.5` 附件 `BUILD-INFO.json`
的 SHA-256 与 GitHub 上传摘要一致，实际交付源码为
`fe5d25fe6d0b320d77e9fe523be26c7f3ac4a5c5`。比较到 main
`3f38b72f919f516baaa0dbd191283bf6db779825`，包含 4 项用户可见修复，以及 Provider
严格 lint 测试修正和发布协作规则更新，准备补丁版 `0.1.6`。

版本准备与覆盖率测量提交为 `315b94e374a98282d91ddd365376a5040368cd1a`。
后续只增加本报告、共享证据及文档索引；最终标签、源码、工作流提交与运行链接以正式附件
`BUILD-INFO.json` 和 Release Notes 为准。

## 更新内容

- **启动连接失败与立即重试。** 初次直连或账户中继探测失败时进入 error 并保留失败原因，
  显示可用的重试入口；Workspace／Agent 页重试立即验证并恢复连接，复用正在进行的请求。
  保留后台自动恢复，一条失败探测不会覆盖另一条已成功连接。
  见 [PR #342](https://github.com/ait-app/ait/pull/342)。
- **Codex Normal／Fast 直接切换。** 桌面与移动端点击速度按钮即可切换，Fast 使用黄色闪电。
  目录合并 `priority`／`fast` 别名，旧保存值仍显示 Fast；仅支持 `priority` 或 `fast_mode`
  的旧原生目录保留传输兼容。见 [PR #343](https://github.com/ait-app/ait/pull/343) 与
  [ADR-119](../../decisions/providers/adr-119-native-account-usage-and-codex-speed.md)。
- **OpenCode ACP 错误与回合恢复。** 已知错误继续使用安全分类说明；其他原生错误保留首行、
  限制正文长度并脱敏常见敏感字段，改善通用失败提示。切换到不再提供思考档位的模型时，
  跳过过期的 thought level，允许后续回合恢复。
  见[源码提交](https://github.com/ait-app/ait/commit/0af74ab91e7567add86fcd2f00d61c991757dc11)。
- **分叉聊天的有界传递与首次发送压缩。** 移除原先的硬性上下文大小拒绝，优先保留最近原文，
  为较早对话生成分块摘要；转移时尚无可用模型可继续，在新会话首次发送时按已选模型压缩。
  见 [PR #345](https://github.com/ait-app/ait/pull/345)。

[PR #347](https://github.com/ait-app/ait/pull/347) 同步修正 Provider 测试的严格 clippy 断言，
不单独计为产品功能。[PR #348](https://github.com/ait-app/ait/pull/348) 移除发布后的强制
人工 review／二次确认：完成质量与附件检查后自动正式发布，实测覆盖率要求保持有效。

## 兼容性与限制

分叉上下文的转移文本预算为 512 KiB，并以明确标记省略更早内容；编码后的响应还受
850 KiB 限制。首次发送时，超过 48 KiB 的聊天正文会分块摘要（每块至多 92 KiB），
保留最近约 32 KiB 原文。摘要会调用所选模型；无法压缩时保留近期原文，并明确标记
较早内容已省略，不能将转移上下文视为完整聊天存档。

新增 `agent.context.compact.request` 需要新版 daemon；旧 daemon 或所选模型不支持压缩时，
客户端走上述保留近期原文的降级路径。建议客户端与 daemon 同时升级。
真实认证 Provider、不同模型上下文预算和实体移动设备尚未在本次本机验证。

桌面标签仅构建 Linux x86_64 与 macOS 13+ Apple Silicon 安装包、更新元数据和校验文件，
不触发 Android APK、Google Play、TestFlight 或 AUR。macOS 正式 App 必须通过
Developer ID 签名、公证及成品 daemon 生命周期门禁；Nightly 的 ad-hoc 签名与未公证状态
不能替代正式包验证。

## 发布准备与验证

同步 15 个 Cargo workspace 包、根 npm 包和 6 个 npm workspace、两份锁文件及根
`PKGBUILD` 为 `0.1.6`。结构化比对确认相对 main 准备基线的第三方依赖记录未变，
npm 锁文件保留原格式。本次准备只更新版本、changelog、报告与索引，没有新增生产代码修复。

本地验证：

- `npm ci --no-audit --no-fund`、发布版本与本地 workspace 链接检查通过。
- 发布脚本 53 项、移动发布脚本 41 项通过。
- `npm run build:desktop-main`、桌面与移动端类型检查通过。
- 移动端 8 个相关文件 161 项、启动导航 31 项、协议附件 13 项、SDK daemon client 165 项定向测试通过。
- 本版 daemon 构建、组件方法安装与生产 RPC 目录两个定向测试通过。
- provider／api／domain／filesystem 四个 crate 的 all-targets clippy（`-D warnings`）通过。
- 隔离离线 Provider 的浏览器 E2E 4 项通过：认证、Workspace 展示、原生 Codex 回合与配置 API。

全仓格式检查、文档索引与链接检查、Rustfmt 和 diff 空白检查通过。适用的 PR／main CI 与两平台正式构建
均须通过；正式附件生成后核对实际字节摘要、更新元数据、构建来源，以及 ZIP／DMG 内
App 的签名和公证票据，并将实际结果写入 Release Notes。

按[发布指南](../../operations/releasing.md#自动完成正式发布)自动完成正式发布，不在 Draft
等待人工确认。稳定版发布后复核 Latest 指向 `v0.1.6`、非 Draft 状态、正文和附件完整性。

## Test coverage

本次实际测量 **provider、api、domain、filesystem 四个 Rust crate、默认 features**
（包括 workspace serde_json `float_roundtrip`），合计 **90.2478%（41875/46400）行覆盖率**。
不是整个 Rust workspace 或整个产品覆盖率。测量提交为
`315b94e374a98282d91ddd365376a5040368cd1a`。

| Crate      | 覆盖行／总行 | 行覆盖率 |
| ---------- | ------------ | -------- |
| provider   | 29876/32383  | 92.2583% |
| api        | 2195/3372    | 65.0949% |
| domain     | 536/556      | 96.4029% |
| filesystem | 9268/10089   | 91.8624% |

测试数量单列：**1412 通过、0 失败、31 项既有忽略**；维持 nextest 默认并行度，没有降低
并发、拆出失败用例重跑或新增豁免。忽略项来自 Provider 的原生／认证环境要求，名称和原因
见[共享 JSON 证据](release-0.1.6-coverage.json)。JSON 同时记录逐文件计数、LCOV 零执行行、
全部 Rust 源码与构建输入／fixture 指纹，用于核对最终发布源码等价性。

没有额外文件排除，沿用 cargo-llvm-cov 默认测试源码过滤。两个直接相关 daemon 注册测试
和客户端／浏览器测试单独执行，未混入上述 Rust instrumentation 数据；JavaScript／TypeScript
行覆盖率未测。本次未手动运行全 Rust workspace 测试或覆盖率。

精确命令如下。使用从既有 target APFS clone 的独立目录；clean 清除旧 instrumentation
产物，不执行测试。实际执行与报告仅选择这四个 crate。

```sh
export CARGO_TARGET_DIR="$PWD/target/release-0.1.6-coverage"
cargo llvm-cov clean --workspace --locked --offline
cargo llvm-cov nextest -p provider -p api -p domain -p filesystem --locked --offline --html --status-level all
cargo llvm-cov report -p provider -p api -p domain -p filesystem --json --output-path /tmp/ait-release-0.1.6-coverage-raw.json
cargo llvm-cov report -p provider -p api -p domain -p filesystem --lcov --output-path /tmp/ait-release-0.1.6-coverage.lcov
```

平台为 macOS 27.0.1 arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、nextest 0.9.148。
HTML 位于 `target/release-0.1.6-coverage/llvm-cov/html/index.html`；共享 JSON 提供不依赖
本机路径的计数和定位。百分比以 LLVM JSON summary 为准，LCOV `DA=0` 仅用于定位。

API crate 本次为 65.0949%，未借用 daemon 等未测包的集成测试贡献。重要未覆盖行为包括
API 连接／鉴权／背压与部分 MCP 路径、分叉上下文响应预算与异常分支、Provider 原生进程／
协议失败和 filesystem 的异常返回；后续修改这些路径时应补定向故障注入。
Linux／Windows 条件分支、外部认证 Provider、Python／shell fixture、doctest、Kotlin／原生
移动端不在本次 instrumentation 范围内。上一版 0.1.5 选择 browser／provider／metadata，
与本次范围及跨 crate 测试贡献不同；没有可比基线，不计算百分点变化。
