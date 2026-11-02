# Ait 0.0.10 发布说明

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-09-30（Asia/Shanghai）。准备基线为 main
[`f3181f15`](https://github.com/necokeine/ait/commit/f3181f1567d991fd8530be59236635216fadfc85)。
本版承接 0.0.9，正式源码对应新标签 `v0.0.10`；实际应用提交、工作流提交与构建链接
随 Release 的 `BUILD-INFO.json` 发布。

**此版本未发布安装包。** [首次构建](https://github.com/necokeine/ait/actions/runs/36658321678)
在双平台成品启动测试中发现目录更新事件名未映射到客户端协议，Release 上传步骤未执行。
保留 `v0.0.10` 标签；修复后的正式版本为 [0.0.11](release-0.0.11.md)。

## 更新内容

- **GitLab 支持。** 根据仓库远端识别 GitLab.com、自建实例、SSH alias 和多层 group，
  使用 `glab` 读取及操作 MR、讨论和流水线；支持从 MR 创建 worktree。
  GitLab 仓库不会再被错误发送 GitHub 专用查询。
  [PR #132](https://github.com/necokeine/ait/pull/132)
- **目录同步与活动状态。** 改进项目、工作区及 Agent 的分页和订阅同步，保留稳定游标与
  locale 排序；工作区状态综合 Agent、子任务和终端活动，补充终端关注状态与会话事件。
  [PR #131](https://github.com/necokeine/ait/pull/131)
- **会话与时间线兼容。** 合并工具生命周期、同消息助手片段及推理片段，搜索和分叉复用
  展示投影；改进历史搜索、会话恢复及审批等待行为，恢复失败时保留原有配置和归档状态。
  [PR #131](https://github.com/necokeine/ait/pull/131)
- **工作区创建与清理。** 支持创建工作区时初始化 Agent，改进幂等重试、首条输入、PR 检出、
  setup 和归档资源清理。跨仓库 fork 检出的 setup 仍需显式批准。
  [PR #131](https://github.com/necokeine/ait/pull/131)

## 升级与兼容

[Release 下载页](https://github.com/necokeine/ait/releases/tag/v0.0.10)。平台延续当前发布流程：

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.10-macos-arm64.dmg`、`Ait-0.0.10-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.10-linux-x64.tar.gz` |

退出旧应用，更新完整安装包后重新打开。完整 GitLab 面板及新能力协商需要同时更新桌面
客户端与 server；只更新远端 server 不会更新本机界面。应用沿用 Ait 用户数据目录和认证，
不自动迁移或删除 Paseo 数据，不恢复已退役的 relay、Plugin、Hub、Chat 或 Loop 能力。

GitLab 操作需要在运行 server 的 Host 安装 `glab` 并登录目标域名：

```sh
glab auth login --hostname gitlab.com
# 自建实例使用实际域名：
glab auth login --hostname code.company.example
```

Ait 复用 CLI 登录配置，不另存 token。自建域名登录后，未知 host 的失败缓存最多需 30 秒
过期，也可重启 server。当前支持已在本地的 GitLab 仓库；GitHub 专用项目目录/克隆接口
没有扩展为 GitLab 目录接口。详见 [GitLab 验证与限制](../../../reports/workspace/gitlab-forge.md)。

## 发布验证

版本提交仅同步版本元数据、锁文件与文档：12 个 Cargo 包、28 处本地版本约束、
根 npm 包和 6 个 `@ait/*` workspace 一致为 `0.0.10`，第三方依赖保持基线锁定版本。

基线 [main CI](https://github.com/necokeine/ait/actions/runs/36561148957) 已通过。
下列本地检查在 macOS arm64、`f3181f15` 加本次版本变更上完成：

| 检查                                                                                                              | 结果                             |
| ----------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `npm ci --offline --no-audit --no-fund`                                                                           | 锁定依赖安装和项目补丁通过       |
| `npm run verify:release -- v0.0.10`、`npm run verify:local-packages`                                              | 版本与本地依赖链接校验通过       |
| Cargo/npm 锁文件语义比较                                                                                          | 仅本地包版本变化，第三方依赖未变 |
| `npm run test:release`                                                                                            | 12 passed，0 failed              |
| `npm run build:desktop-main`                                                                                      | 共享包与 Electron 主进程构建通过 |
| `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`                                              | 通过                             |
| `npm run test --workspace=@ait/mobile -- --project unit native-release-version.test.ts src/changelog`             | 3 文件、46 passed，0 failed      |
| `CARGO_TARGET_DIR=/private/tmp/ait-release-0.0.10-target cargo build --locked --offline -p daemon --bin daemon`   | 通过，版本输出 `server 0.0.10`   |
| `AIT_SERVER_BIN=/private/tmp/ait-release-0.0.10-target/debug/daemon node apps/desktop/scripts/prepare-server.mjs` | 版本与单 server 资源暂存通过     |
| `cargo fmt --all --check`、改动 JSON/Markdown 格式、文档链接与 `git diff --check`                                 | 通过                             |

本地 debug server 仅用于验证，不作为正式发布资产。

版本 PR 和合并后的 main CI 通过后，创建不可变标签 `v0.0.10`。Release Ait 工作流
构建原生 Linux/macOS 安装包，验证资源布局、成品启动及更新文件摘要；macOS 额外要求
签名与 Apple 公证。双平台均成功后上传 10 个资产：四个安装包、两个 macOS blockmap、
两个更新 YAML、`BUILD-INFO.json` 和 `SHA256SUMS`。

最终发布状态及构建链接以 [Release 页面](https://github.com/necokeine/ait/releases/tag/v0.0.10)
为准。发布后核对源码与标签、完整资产集合、SHA-256 和更新元数据。
完整操作步骤见[发布指南](../../../operations/releasing.md)。

## Test coverage

**本次未测量行覆盖率**：发布提交只改版本元数据和文档，没有 Rust 或前端运行代码变更，
按仓库规则跳过本地 Rust 测试与覆盖率。构建和测试通过数量不作为覆盖率百分比。

原功能验证见 [GitLab 报告](../../../reports/workspace/gitlab-forge.md)与[会话 API 提交验证](../../../reports/daemon/paseo-server-pr-validation-2026-09-29.md)，
其中的历史测量不标记为本次发布测量。真实 GitLab 企业实例、SSO/代理、远端操作和用户
界面的人工验收不由成品启动测试代替；后续行为变更按仓库规则补充定向测试和覆盖率。
