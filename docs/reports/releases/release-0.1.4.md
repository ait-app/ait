# Ait 0.1.4 Draft 发布说明

日期：2026-10-15（Asia/Shanghai）。准备基线为 main
`119f96b10229cbeac672954ad85f1db6d13de4a2`；版本与发布规则准备提交为
`23467388`。每日发布将修复计入发布条件，本次准备补丁版本 `0.1.4`。
上一正式版 `0.1.3` 已同版本重建，实际交付源码为
`9b5d965440744814daa7af62c433abc6bf240df8`，由其公开附件 `BUILD-INFO.json`
确认；本次据此比较，不重复列入已交付的 #327、#328、#329。

## 更新内容

- **所有 Provider 的上下文压缩提示。** Codex、Claude、OpenCode、DeepSeek Harness
  （原生和 ACP）与 Antigravity 将原生压缩事件统一显示为进行中、完成或失败。
  压缩标识用于匹配重叠事件并去重，保留失败结果与历史回放；不依据普通输出文本或
  token 数量猜测状态，也不改变 Provider 压缩策略或增加模型调用。
  见 [PR #317](https://github.com/ait-app/ait/pull/317) 与
  [ADR-150](../../decisions/providers/adr-150-provider-compaction-lifecycle.md)。
- **用量 Host 刷新。** 连接状态变化后更新所选用量 Host，修复 React Compiler 优化导致
  状态停留在初次渲染的问题。见 [PR #330](https://github.com/ait-app/ait/pull/330)。
- **移动端状态栏。** 原生状态栏图标随应用明暗主题切换。
  见 [PR #332](https://github.com/ait-app/ait/pull/332)。
- **官方服务 HTTPS 端口。** 默认账户服务使用 `https://aitapp.cn/api`，设备验证使用
  `https://aitapp.cn/auth/device`；迁移保存在旧官方 `:8443` 地址的配置，保留自定义
  服务地址。见 [PR #333](https://github.com/ait-app/ait/pull/333)。

## 安装与 review 范围

预期桌面附件共 10 个：Linux x86_64 AppImage／tar.gz、macOS Apple Silicon DMG／ZIP，
两份 blockmap、两份更新元数据、`BUILD-INFO.json` 和 `SHA256SUMS`。
macOS 需要 13 或更新版本；正式构建必须通过 Developer ID 签名、公证和成品 daemon
生命周期门禁。Nightly 仍为 ad-hoc 签名且未公证。

压缩提示依赖原生 CLI 发送对应事件；旧 CLI 可能需要升级。建议 daemon 与客户端同时升级，
以支持失败状态和压缩标识。只有历史完成事件的旧记录不会补造进行中提示。
移动端状态栏修复需要另行分发 Android／iOS 应用；本次桌面标签不触发 Android APK、
Google Play、TestFlight 或 AUR 发布。未合并的 #334、#331 等不纳入本版本。

按[发布指南](https://github.com/ait-app/ait/blob/fe5d25fe6d0b320d77e9fe523be26c7f3ac4a5c5/docs/operations/releasing.md#draft-后必须停下等待人工-review)，完成 CI、
安装包与元数据校验后保持 Draft，公开 Latest 继续为 `v0.1.3`，等待用户 review。
最终源码、工作流提交与运行链接以该 Draft 附件 `BUILD-INFO.json` 为准。

## 发布准备与门禁

Rust workspace 与 15 个本地 Cargo 包、根 npm 包和 6 个 workspace、锁文件及根
`PKGBUILD` 同步为 `0.1.4`。结构化比对确认第三方依赖锁定记录未变。
准备变更限于版本、changelog、发布指南与覆盖率证据；不修改 Rust 源码、CI 或审批环境。

本地验证通过：

- `npm ci --no-audit --no-fund` 安装 2288 个锁定包；版本一致性和本地 workspace 链接检查通过。
- `npm run test:release` 53 项、`npm run test:mobile-release` 41 项全部通过。
- `npm run build:desktop-main` 与桌面／移动端类型检查通过。
- 协议 wire compatibility 14 项、账户会话 66 项，以及 Host 连接状态、压缩标签、stream
  和 replica cache 四个移动端文件 118 项定向回归全部通过。
- Oxfmt 检查 3356 个文件；变更 UI lint 无需检查 JS／TS；文档检查 1 项及 372 份 Markdown 链接通过。
- `cargo metadata --locked --offline --no-deps --format-version 1` 确认 15 个包版本一致；
  `cargo fmt --all --check`、`bash -n PKGBUILD` 与 `git diff --check` 通过。

准备 PR、main 与正式发布构建仍由 CI 验证。最终构建、签名、公证和成品门禁链接在
GitHub Draft 中补充；本地脚本结果不代替安装包门禁。

## Test coverage

用户明确要求全 Rust workspace；本次实测行覆盖率为
**95.2744%（64617/67822）**，范围为全部 **15 个 crate、默认 features**。
测量提交为 `2346738823e1593739af12888df59d6956f214da`；后续准备变更只增加文档和证据，
[共享 JSON](release-0.1.4-coverage.json)记录全部 Rust 源码、manifest、锁文件与 fixture
指纹，可核对最终发布源码等价性。包含逐文件计数、LCOV 零执行行和忽略用例原因。

| Crate       | 覆盖行／总行 | 行覆盖率  |
| ----------- | ------------ | --------- |
| api         | 3195/3371    | 94.7790%  |
| bot         | 2142/2250    | 95.2000%  |
| browser     | 715/733      | 97.5443%  |
| daemon      | 2288/2407    | 95.0561%  |
| domain      | 556/556      | 100.0000% |
| filesystem  | 9599/10089   | 95.1432%  |
| host-link   | 192/198      | 96.9697%  |
| metadata    | 7010/7372    | 95.0895%  |
| model       | 1413/1465    | 96.4505%  |
| persistence | 1846/1927    | 95.7966%  |
| provider    | 30519/32086  | 95.1163%  |
| relay       | 666/698      | 95.4155%  |
| schedule    | 998/1041     | 95.8694%  |
| terminal    | 1610/1666    | 96.6387%  |
| voice       | 1868/1963    | 95.1605%  |

测试执行结果单列：**2384 通过、0 失败、32 跳过**，维持 nextest 默认并行度。
既有跳过用例依赖原生或认证 Provider 环境，以及 fish 4；完整名称和原因见共享 JSON。
未增加忽略项、额外文件排除或覆盖率豁免，沿用 cargo-llvm-cov 默认测试源码过滤。
这不是 JavaScript／TypeScript 或整个产品的覆盖率；客户端定向回归仅报告测试结果。

测量平台为 macOS 27.0.1 arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、nextest 0.9.148。
Linux／Windows 条件分支、外部 CLI、Python／shell fixture 和 doctest 未纳入本机 instrumentation。
没有上一正式版实际源码的可比测量：旧报告的 Rust 指纹与该源码不同，因此不计算百分点变化。

精确命令如下，均在测量提交运行。`SHERPA_ONNX_LIB_DIR` 使用本机已有的静态库；
其他平台须替换为对应平台库。先将旧依赖缓存克隆到独立构建目录，再清理 workspace
instrumented 产物，避免旧覆盖率数据混入本次结果。

```sh
export CARGO_TARGET_DIR="$PWD/target/release-0.1.4-coverage"
export SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib
cargo llvm-cov clean --workspace --locked --offline
cargo llvm-cov nextest --workspace --locked --offline --html --status-level all
cargo llvm-cov report --json --output-path /tmp/ait-release-0.1.4-coverage-raw.json
cargo llvm-cov report --lcov --output-path /tmp/ait-release-0.1.4-coverage.lcov
python3 scripts/check-crate-coverage.py /tmp/ait-release-0.1.4-coverage-raw.json
```

HTML 报告生成在 `target/release-0.1.4-coverage/llvm-cov/html/index.html`；共享 JSON
提供不依赖本机路径的计数与定位。百分比来自 LLVM JSON summary，LCOV `DA=0` 仅作定位，
不用于重算分母。最后的手动逐 crate 95% 检查退出 1：`api` 为 **94.7790%**，
其余 14 个 crate 达到 95%；这不表示覆盖率收集或测试失败，也不声明全部 crate 达标。

重要缺口包括 API 单连接调度、Workspace 清理、MCP 工具错误路径，以及 Git 操作失败、
Provider 原生错误和离线语音异常。后续修改这些路径时应补对应故障注入；Linux／Windows
与需要认证的真实 Provider 用例仍需相应平台和隔离凭据环境验证。
