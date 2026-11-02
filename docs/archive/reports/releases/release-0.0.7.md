# Ait 0.0.7 发布准备

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

基线：`80e97c0` 加当前工作区；本报告描述发布准备，不表示已经创建标签或发布 GitHub Release。
决策见 [ADR-053](../../../decisions/clients/adr-053-paseo-desktop-release.md)。

## 已完成

- 活跃 Cargo/npm workspace 版本和锁文件统一为 0.0.7；更新应用内 CHANGELOG。
- Release 与桌面 CI 切换到 `apps/desktop`，前端导出来自 `apps/mobile`。
- 平台保持 Linux x86_64 与 macOS arm64；Windows、deb/rpm 不进入发布目标。
- 构建范围固定为 `daemon --bin daemon`。暂存验证版本并清理旧 sidecar；安装包的
  `resources/bin/` 必须只有一个可执行文件 `server`，ASAR 不能包含 Node server/CLI 包。
- 正式应用 ID 采用 `dev.ait.desktop`；macOS 为 server 签名并要求公证。
- 安装包、更新 YAML、blockmap 一同收集，检查 SHA-512/版本/引用，再生成覆盖全部资产的 SHA256SUMS。
- 成品冒烟测试验证自动启动、鉴权 RPC、服务重启与重连、应用退出后的进程回收。

## Test coverage

Rust 行覆盖率：**不适用，本次未修改 Rust 源代码或运行行为**；只更新 Cargo 版本约束和锁文件。
按 AGENTS.md 跳过 Rust workspace tests 与 llvm-cov。没有把测试通过数量当作覆盖率。
JavaScript/TypeScript 行覆盖率未测量；本轮验证范围是发布故障门禁、桌面回归与成品运行。

| 验证                                                                                                                          | 结果                                                                                        |
| ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `npm run verify:release -- v0.0.7`                                                                                            | 9 个活跃 npm 包、lockfile、Cargo workspace 和标签一致                                       |
| `npm install --package-lock-only --ignore-scripts --offline --no-audit --no-fund`                                             | npm 锁文件解析通过，无依赖升级                                                              |
| `npm run test:release`                                                                                                        | 9 项通过；覆盖旧二进制残留、错版 server、执行权限、缺失资产、额外平台资产、更新摘要和校验和 |
| `AIT_SERVER_BIN=.../target/release/daemon npm test --workspace=@getpaseo/desktop`                                             | 389 项通过，9 项按平台或环境跳过；真实 Rust 子进程测试启用                                  |
| `npm run test --workspace=@getpaseo/app -- src/i18n/resources.test.ts src/runtime/rust-daemon native-release-version.test.ts` | 70 项通过                                                                                   |
| `npm run typecheck --workspace=@getpaseo/desktop --workspace=@getpaseo/app`                                                   | 通过                                                                                        |
| 变更脚本的 oxfmt、oxlint 与 `git diff --check`                                                                                | 通过                                                                                        |
| `cargo metadata --offline --format-version 1`                                                                                 | 完整 workspace 解析通过并更新锁文件                                                         |
| `cargo build --locked --offline --release -p daemon --bin daemon`                                                             | 通过，产物报告 `server 0.0.7`                                                               |
| `cargo check --workspace --locked --offline`                                                                                  | 通过，完整 Cargo workspace 保持可构建                                                       |
| Electron Web 导出                                                                                                             | 通过，确认嵌入 0.0.7 发布说明                                                               |
| 本地 macOS DMG 与成品 server 生命周期                                                                                         | 通过，renderer error 为 0                                                                   |
| 正式文件布局的本地 macOS DMG/ZIP/更新元数据                                                                                   | 通过；实际 latest-mac.yml 的引用、大小、SHA-512 校验通过                                    |
| ZIP 内容与 DMG 完整性                                                                                                         | ZIP 内只有 `Contents/Resources/bin/daemon`，Bundle ID/版本正确；`hdiutil verify` 通过       |

本机为 macOS arm64。运行时验证使用临时 server 和 Electron 数据目录，结束后清理；没有
执行用户已有的定时任务或修改用户项目。最初受沙箱端口限制的生命周期测试已在允许本机
监听的环境中重跑通过。发布工具的 Node tests 与桌面 Vitest 已分开，避免互相误识别。

本地验证日志在 `.tmp/release-*.log`；DMG、ZIP 和更新资产在 `.tmp/release-validation/mac/`。
这些是临时验证产物，没有上传。正式布局的本地打包命令在生产配置基础上显式覆盖了
`mac.identity=-`、`mac.hardenedRuntime=false`、`mac.notarize=false`，只验证布局和运行能力，
不能代替 Developer ID 签名、公证或正式分发。

## 本地 Developer ID 签名验证

在上述布局验证之后，使用本机 `Developer ID Application: Dong Shan (SVS7GV79T9)`
生成 `apps/desktop/release/signed/Ait-0.0.7-macos-arm64.dmg`。应用和内置 server 启用了
hardened runtime，DMG、应用及 server 均有 Developer ID 签名和时间戳。

- `codesign --verify --deep --strict` 校验应用通过；server 和 DMG 的独立签名校验通过。
- `hdiutil verify` 通过；只读挂载后再次确认应用版本、Bundle ID、签名及 `bin/daemon` 唯一性。
- 正式签名后的成品启动、鉴权 RPC、server 重启重连和退出回收验证通过，renderer error 为 0。
- 本机未配置公证凭据，本次产物未提交 Apple 公证，也未上传 GitHub。
- GitHub 仓库的五项发布 Secrets 名称已核验存在；凭据有效性仍需由正式发布流程验证。

签名产物和验证明细保留在被忽略的本地输出目录，不纳入源码提交。

## PR CI 的 Linux 启动器测试修复

首轮 PR CI 的九项 Linux 启动器测试使用旧的最小安装目录，调用 `afterPack` 时缺少
`packager.appInfo.version`，也没有新发布校验要求的 ASAR、Web 导出及 server，因而在
执行启动器之前失败。依赖安装、版本校验、发布脚本测试、构建和类型检查均已通过。

测试样本现包含真实生成的最小 ASAR、Web 入口、单个可执行 server 及 builder 版本上下文。
新增可跨平台运行的用例，验证旧 sidecar 会阻止包装，而合法安装目录会保留原始可执行文件
并安装 Linux 启动器；九项依赖 Linux `/proc` 的启动行为测试继续由 Linux CI 执行。
生产打包校验保持启用，测试中单独关闭真实 GUI 冒烟入口。

本机针对该文件的测试结果为 1 项通过、9 项因 macOS 跳过；oxfmt、oxlint 和差异空白检查通过。
本次修复仅修改测试样本和报告，没有 Rust 源代码变化，不运行 Rust workspace tests。

## 首次 Release workflow 与 AppImage 收集修复

`v0.0.7` 固定在合并提交 `4583883`。首次发布运行中，Linux 原生编译、AppImage/tar.gz
打包与成品启动测试通过；macOS 的 Developer ID 签名、Apple 公证、启动验证及资产收集均通过。
由于 Linux 资产收集失败，汇总发布步骤未运行，没有创建 GitHub Release。

原因是 electron-builder 针对 AppImage 把 `${arch}` 展开为 `x86_64`，而 tar.gz 使用 `x64`。
收集器误把 AppImage 文件名写为 `Ait-linux-x64.AppImage`，导致文件不存在错误。收集器现改为
匹配实际的 `Ait-linux-x86_64.AppImage`，并保留版本、文件清单、大小与摘要验证。

回归测试使用仓库 builder 配置及其真实的架构/文件名展开方法生成模拟产物，避免收集器和
测试样本共用错误文件名。恢复流程将发布工具与标签源码分别检出，允许从修复分支发起
workflow_dispatch，继续构建原始 `v0.0.7` 而不移动标签。

本次工具修复的 10 项 Node 测试全部通过；未修改 Rust 源代码，按 AGENTS.md 跳过本地
Rust workspace tests，Rust 覆盖率不适用。

## 发布验证范围

- Linux 原生编译、AppImage/tar.gz 生成与 Xvfb 成品启动测试：首轮 Release CI 已通过。
- 使用 GitHub Secrets 完成 macOS Developer ID 签名和 Apple 公证：首轮 Release CI 已通过。
- 双平台资产汇总与实际 GitHub Release 上传：使用修复后的发布工具重跑验证。

工作流使用 `ubuntu-24.04` 和 `macos-15`；后者的 arm64 架构已按
[GitHub 官方 runner 文档](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)核对。

旧桌面数据库不自动迁移到 `~/.ait-server-desktop`，旧文件保留；这项边界已写入 ADR 和
[发布操作指南](../../../operations/releasing.md)。
