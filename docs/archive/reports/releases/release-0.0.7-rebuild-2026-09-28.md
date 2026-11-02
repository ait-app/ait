# Ait 0.0.7 同版本重建发布

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-09-28（Asia/Shanghai）。本次以最新 main 重建现有 0.0.7，覆盖同名安装包和
更新资产，版本号继续为 0.0.7。

## 发布来源

| 项目             | 记录                                                                                                                         |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| 发布页           | [Ait v0.0.7](https://github.com/necokeine/ait/releases/tag/v0.0.7)                                                           |
| 应用与工作流源码 | [`c03c42d4a05b54910463826465ed6efe3027e1ea`](https://github.com/necokeine/ait/tree/c03c42d4a05b54910463826465ed6efe3027e1ea) |
| 发布工作流       | [Release Ait #36338537563](https://github.com/necokeine/ait/actions/runs/36338537563)                                        |
| 上一次安装包源码 | [`3a5b68b6215ced3c8311b90a87fd029c16e45a91`](https://github.com/necokeine/ait/tree/3a5b68b6215ced3c8311b90a87fd029c16e45a91) |
| 发布状态         | 已发布；双平台构建、10 个资产替换及发布后校验全部通过                                                                        |

使用现有工作流的 `source_commit` 入口重建；`v0.0.7` 标签保持原位。
GitHub 自动提供的 Source code 归档仍对应最初标签，需要本次安装包源码时使用上面的提交链接。
下载资产中的 `BUILD-INFO.json` 记录实际应用提交、工作流提交和构建链接。

## 本次更新

- **创建 Git worktree 工作区。** 统一创建入口支持新建分支、checkout 已有分支、从项目根
  或子目录创建，并保留标题与首轮上下文。重复请求及服务重启后的重放不会重复创建或
  重跑 setup。[PR #120](https://github.com/necokeine/ait/pull/120)
- **自动生成标题与 Git 文案。** 接通会话标题、工作区标题、托管占位分支名、提交信息和
  PR 文案的生成流程，支持配置优先级、失败回退，并保护手工标题和显式分支名。
  [PR #118](https://github.com/necokeine/ait/pull/118)
- **修复会话导入能力识别。** 工作区内导入会话不再误提示需要更新 Host；保留搜索条件
  和目标工作区参数。此修复包含前端更新，需要安装并重新打开桌面应用。
  [PR #121](https://github.com/necokeine/ait/pull/121)
- **改善对话和命令显示。** 修复新目录工作区首轮对话、SDK 字段兼容、空会话标题及
  文件订阅更新；Codex 工具输出优先显示解析后的读取、搜索和命令动作。
  [PR #114](https://github.com/necokeine/ait/pull/114)、
  [PR #115](https://github.com/necokeine/ait/pull/115)、
  [PR #116](https://github.com/necokeine/ait/pull/116)
- **统一维护入口。** 移除旧 desktop 与旧 Rust daemon、worker、CLI 实现；发布继续使用
  `apps/desktop`、`apps/mobile` 与独立 Rust server。Rust/UI CI 按实际改动触发，修复离线
  Codex 测试启动方式。[PR #117](https://github.com/necokeine/ait/pull/117)、
  [PR #119](https://github.com/necokeine/ait/pull/119)

本次仍包含此前的 Ait/Paseo 配置、浏览器会话、链接与更新缓存隔离，以及桌面服务监听
设置和经鉴权的 Rust SSH 连接。既有 Paseo 数据不自动迁移或删除。

## 下载与安装

| 平台                                 | 安装包                                                                                                                                                                                          |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS Apple Silicon，macOS 13 或更新 | [DMG](https://github.com/necokeine/ait/releases/download/v0.0.7/Ait-0.0.7-macos-arm64.dmg)、[ZIP](https://github.com/necokeine/ait/releases/download/v0.0.7/Ait-0.0.7-macos-arm64.zip)          |
| Linux x86_64                         | [AppImage](https://github.com/necokeine/ait/releases/download/v0.0.7/Ait-linux-x86_64.AppImage)、[tar.gz](https://github.com/necokeine/ait/releases/download/v0.0.7/Ait-0.0.7-linux-x64.tar.gz) |

**已安装旧版 0.0.7 的用户需要重新下载安装包。** 同版本重建不会作为版本升级自动提示。
先退出旧应用，再替换安装并重新打开；仅重启后台 server 不会加载新的前端导入修复。
macOS 使用已签名、公证的 DMG/ZIP；Linux AppImage 下载后赋予执行权限再启动。
本次不发布 Windows、Intel Mac 或 Linux arm64 包。

如需校验下载文件，取同一 Release 的 `SHA256SUMS`，在安装包所在目录执行：

```sh
# macOS：指定刚下载的文件，避免检查未下载的其他平台资产。
grep '  Ait-0.0.7-macos-arm64.dmg$' SHA256SUMS | shasum -a 256 -c -
# Linux：
grep '  Ait-linux-x86_64.AppImage$' SHA256SUMS | sha256sum -c -
```

Release 同时提供 `latest-linux.yml`、`latest-mac.yml`、macOS blockmap、`BUILD-INFO.json`
和 `SHA256SUMS`。安装包只内置独立的 Rust `server`，不包含旧 daemon/worker/CLI sidecar。
旧桌面 SQLite 数据保留，发布过程不执行旧数据迁移。

## 发布验证

| 检查                                                                      | 结果                                                                                |
| ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `npm run verify:release -- v0.0.7`                                        | 通过；Rust workspace、9 个 npm 包、lockfile 与标签版本一致                          |
| `npm run test:release`                                                    | 12 passed，0 failed；macOS arm64，源码 `c03c42d`                                    |
| [最新 main CI](https://github.com/necokeine/ait/actions/runs/36338256000) | UI 构建、类型检查、Desktop/App 测试通过；本次仅前端变化，Rust 按规则跳过            |
| [最近 Rust CI](https://github.com/necokeine/ait/actions/runs/36336496233) | Linux fmt、Clippy、完整 workspace 测试通过：1266 passed，0 failed，3 项既有 ignored |
| Linux 安装包和成品启动验证                                                | 通过；AppImage、tar.gz 及更新文件生成成功，启动测试 `rendererErrors: []`            |
| macOS 签名、公证和成品启动验证                                            | 通过；DMG、ZIP 完成 Developer ID 签名和 Apple 公证，启动测试 `rendererErrors: []`   |
| 上传后的来源、资产摘要与标签核对                                          | 通过；10 个资产全部替换，SHA-256 与实际构建来源一致，原标签未移动                   |

两个平台均已通过版本校验、发布工具回归、release server 编译、桌面打包、资源布局检查、
成品 server 生命周期冒烟测试和更新文件摘要验证，之后覆盖 Release 资产。
macOS 的正式打包还通过了 Developer ID 签名与 Apple 公证。

[发布资产校验记录](release-0.0.7-rebuild-2026-09-28-assets.json)记录 10 个资产的大小、
SHA-256、上传时间及实际构建来源。发布后下载了 `BUILD-INFO.json`、`SHA256SUMS` 和
两个更新 YAML，逐一验证摘要；清单中的 9 项摘要与 GitHub 资产摘要一致，
`SHA256SUMS` 自身摘要也与 GitHub 一致。两个更新 YAML 的版本、文件名和大小均与
新资产一致，原有标签的对象及目标提交均未变化。

安装包的原生构建、签名、公证、资源和启动检查在发布工作流中完成；本地没有再次下载
安装包或人工安装验证。上述校验范围不包含真实用户环境的端到端验收。

实际触发命令：

```sh
gh workflow run release.yml --ref main \
  -f tag=v0.0.7 \
  -f source_commit=c03c42d4a05b54910463826465ed6efe3027e1ea
```

## Test coverage

**本次未重新测量行覆盖率**：本次工作仅为既有提交重建与发布文档，未修改 Rust 或前端
运行代码；按仓库规则不重复执行 Rust 全量测试与覆盖率。没有把打包成功或测试数量当作覆盖率。

参考此前 [`795869b`](https://github.com/necokeine/ait/commit/795869befeab54e0cc8f22b15a08fe657eed398a)
的完整 workspace 测量：**91.60%（35,725 / 39,000 行）**，macOS arm64、默认 features，
比当时同范围 main 基线高约 0.04 个百分点。此为已归档结果，不是本次发布的新测量。
已核对发布源码中 246 个被测 Rust 文件、22 个变更 Rust 文件及 Python fixture 的 SHA-256，
均与该制品一致。完整命令为：

```sh
CARGO_TARGET_DIR=/Users/necokeine/Documents/ait/target cargo llvm-cov --workspace --html -- --test-threads=4
```

[覆盖率制品](https://github.com/necokeine/ait/blob/c03c42d4a05b54910463826465ed6efe3027e1ea/docs/reports/workspace-create-worktree-coverage.json)
记录逐文件摘要、范围及命令；[验证报告](../../../reports/workspace/workspace-create-worktree.md)记录相关 crate 明细。
无显式文件排除，测试文件按工具默认规则处理，辅助模块 `test_support.rs` 计入统计；
3 项真实 Provider 测试保持忽略，Python fixture 和 doctest 未插桩。Linux/Windows 行覆盖率
及 JavaScript 行覆盖率未测量；真实付费模型、远端 Forge 和真实用户会话导入未做发布验收。

Worktree 的 PR/Forge checkout 暂不支持。元数据生成的模型可用性与输出质量仍取决于本机
Provider 配置；少数 registry/I/O/rollback 错误映射不在已测覆盖路径中。
