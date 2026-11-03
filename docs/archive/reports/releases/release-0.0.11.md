# Ait 0.0.11 发布说明

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-09-30（Asia/Shanghai）。基于 main `6b5f8cb1` 准备，正式来源以不可变标签
`v0.0.11` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- 修复 Rust `workspace.update`、`agent.update` 事件到客户端 `workspace_update`、
  `agent_update` 的映射，恢复工作区和 Agent 的实时目录更新；保留订阅 ID、generation、
  seq 及 upsert/remove 数据。
- 包含 [GitLab 支持（#132）](https://github.com/necokeine/ait/pull/132)：MR、讨论、流水线及
  MR worktree 检出；支持 GitLab.com、自建域名、SSH alias 和多层 group。
- 包含 [会话与工作区兼容改进（#131）](https://github.com/necokeine/ait/pull/131)：目录分页和
  同步、活动状态、时间线投影、历史搜索、会话恢复、审批等待、初始 Agent 创建和归档清理。

  0.0.10 的 Rust/UI CI 通过，但[正式构建](https://github.com/necokeine/ait/actions/runs/36658321678)
  在成品启动时发现上述事件映射遗漏，双平台均未通过，Release 上传步骤未执行。
  0.0.11 承接这些功能和修复，保留原标签；上一个已发布版本为 0.0.9。

## 安装与升级

[Release 下载页](https://github.com/necokeine/ait/releases/tag/v0.0.11)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.11-macos-arm64.dmg`、`Ait-0.0.11-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.11-linux-x64.tar.gz` |

退出旧应用，更新完整安装包后重新打开。完整 GitLab 界面及新能力需要同时更新桌面客户端
和 server。GitLab 操作需要在运行 server 的 Host 安装 `glab` 并登录目标域名：

```sh
glab auth login --hostname gitlab.com
# 自建实例使用实际域名：
glab auth login --hostname code.company.example
```

Ait 复用 CLI 登录配置。自建域名登录后，失败缓存最多需 30 秒过期，也可重启 server。
当前支持本地已有 GitLab 仓库；GitHub 专用项目目录/克隆接口尚未扩展为 GitLab 目录接口。
跨仓库 fork 检出的 setup 仍需显式批准。具体范围见 [GitLab 报告](../../../reports/workspace/gitlab-forge.md)。

## 发布验证

回归测试先确认未映射事件会被协议拒收，再验证修复后能正确解析；真实 Rust server 集成
测试通过工作区和 Agent 改名验证订阅更新能够到达 SDK。完整成品测试还覆盖 server 启动、
重启、离线 Provider 会话、流式消息及冷启动持久化。

本地验证结果：

- 相关 App 测试 7 个文件、93 passed，包括真实 server 的目录订阅更新集成测试。
- 发布工具测试 12 passed；版本、本地包链接和锁文件语义校验通过。
- Web 导出、Electron 主进程构建、App/desktop 类型检查、格式和 lint 通过。
- 锁定依赖离线 server 构建及资源暂存通过，版本输出 `server 0.0.11`。
- 完整 macOS arm64 本地应用包启动测试通过，`rendererErrors: []`，覆盖会话、重启和持久化。

本地包使用临时签名，仅用于验证；正式安装包由下面的发布工作流签名、公证和上传。

版本提交同步 12 个 Cargo 包、本地 path 版本约束、根 npm 包和 6 个 workspace；第三方依赖
继续使用已有锁定版本。正式发布仍需通过 PR/main CI 和双平台打包；macOS 必须签名并完成
Apple 公证，双平台必须通过成品启动门禁。

发布资产为四个安装包、两个 macOS blockmap、两个更新 YAML、`BUILD-INFO.json` 和
`SHA256SUMS`。上传后核对来源提交、标签、完整资产集合、SHA-256 和更新元数据。
最终状态与工作流链接见 [Release 页面](https://github.com/necokeine/ait/releases/tag/v0.0.11)。
操作步骤见[发布指南](../../../operations/releasing.md)。

## Test coverage

本次没有 Rust 源码变更，按仓库规则跳过本地 Rust 测试与覆盖率。前端修复有定向协议测试、
真实 server 订阅集成测试和成品启动验证；未测量行覆盖率，不把通过数量标记为覆盖率百分比。
真实 GitLab 企业实例、SSO/代理和用户界面的人工验收范围见原功能报告。
