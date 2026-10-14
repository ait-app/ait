# Ait 0.1.3 同版本重建 Draft 准备

日期：2026-10-15（Asia/Shanghai）。用户要求从最新 main 再次构建 `0.1.3`，
并明确要求先到 Draft。准备基线为 `ed415678aa55eabca77b56454e8ed0e8e99428cf`，
包含首次 `v0.1.3` 标签之后合并的 [PR #327](https://github.com/ait-app/ait/pull/327)
和 [PR #328](https://github.com/ait-app/ait/pull/328)。

## 纳入修复

- **Codex computer use 批准。** Auto-review 和 Full Access 自动处理原生 computer use
  的空访问批准请求，恢复原生工具预批准设置；显式工具覆盖、需要填写内容的表单、
  认证交互与 Bot 自动拒绝规则继续生效。见
  [ADR-149](../../decisions/providers/adr-149-codex-computer-use-access-auto-approval.md)。
- **启动 Timeline 与草稿标签。** 每个连接最多同时读取 4 个 Timeline 页面，其他请求
  排队且相同读取合并；等真实 Timeline epoch 到达后再查询 Chat outline，避免启动队列
  拒绝和针对 `tab_<id>` 的 `AgentNotFound`。见
  [安装版诊断与定向回归](../clients/ait-0.1.3-startup-diagnostics-2026-10-14.md)。
- **Codex 模型发现诊断。** 原生模型目录刷新 timeout 保留安全告警，但不再作为当前轮次
  的执行失败原因；普通执行超时继续按执行失败处理。
- **Headless 启动失败恢复。** 在装配后台服务前验证登录与安装身份，失败启动能够释放
  实例锁。#327 同时增加后端适配器与生命周期测试；其
  [覆盖率报告](../daemon/crate-coverage-95-2026-10-14.md)属于该实现任务，不能作为重建源码
  的新覆盖率测量。

首次 Draft 的 MCP、DSH 情景模式、配额／余额和 Workspace 修复继续包含在本版本中，
完整功能范围见 [首次发布说明](release-0.1.3.md)。本次准备只更新 changelog 和文档，
不修改生产代码、版本、依赖锁定记录、CI 或审批环境。

## 重建与 review 边界

使用现有 `release.yml` 的 `source_commit` 输入，从准备 PR 合并后的最新 main 完整
40 位 SHA 构建。两平台与 Release 汇总步骤使用同一固定源码提交；`v0.1.3` 标签继续指向
`91ae1220892f36144be0c532b2c175ede9487ba8`，不移动或重新创建标签。

预计覆盖现有 Draft 的 10 个桌面附件：Linux x86_64 AppImage／tar.gz、macOS Apple Silicon
DMG／ZIP、两份 blockmap、更新元数据、`BUILD-INFO.json` 和 `SHA256SUMS`。
正式 macOS 构建必须通过 Developer ID 签名、公证与成品 daemon 生命周期门禁。
本次不触发 Android APK、Google Play、iOS TestFlight 或 AUR。

同版本重建不会向已安装旧 `0.1.3` 的用户提示版本升级；review 时须重新下载并替换安装包。
GitHub 自动提供的 Source code 归档仍对应原标签，重建源码应使用 `BUILD-INFO.json` 中的
`sourceCommit` 链接。

按[发布协作规则](../../operations/releasing.md#draft-后必须停下等待人工-review)，
完成两平台构建和附件验证后保持 Draft，公开 Latest 继续为 `v0.1.2`，等待用户 review。
最终构建链接、源码／工作流 SHA 与附件验证结果记录在
[GitHub Draft](https://github.com/ait-app/ait/releases)说明和 `BUILD-INFO.json` 中。

## 发布验证

准备工作树基于上述 `ed415678`，本地检查结果如下：

| 命令                                                                 | 结果                                                  |
| -------------------------------------------------------------------- | ----------------------------------------------------- |
| `npm ci --no-audit --no-fund`                                        | 安装 2288 个锁定包                                    |
| `npm run verify:release -- v0.1.3`、`npm run verify:local-packages`  | 根 npm 与 6 个 workspace、锁文件和本地链接一致        |
| `npm run test:release`、`npm run test:mobile-release`                | 53 项、41 项全部通过                                  |
| `npm run build:desktop-main`                                         | 桌面主进程与共享 UI 依赖构建通过                      |
| `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile` | 两个客户端类型检查通过                                |
| `npm run check:format`、`npm run check:ui`                           | Oxfmt 检查 3353 个文件；无修改的 JS／TS 文件需要 lint |
| `npm run check:docs`                                                 | 1 项测试和 370 份 Markdown 本地链接验证通过           |
| `cargo metadata --locked --offline --no-deps --format-version 1`     | 15 个 workspace 包均为 `0.1.3`                        |
| `cargo fmt --all --check`、`bash -n PKGBUILD`、`git diff --check`    | 通过                                                  |

准备 PR 和 main CI 另行核验；发布工作流完成前不将安装包构建、签名、公证或成品门禁
写为通过。最终附件结果在 GitHub Draft 中补充。

## Test coverage

**Not measured。** 本次仅准备既有 main 的同版本重建、changelog 与文档，未修改
`bins/` 或 `crates/` 的 Rust 源码，按仓库要求跳过本地 Rust 测试与覆盖率。
既有 PR 和 main CI 结果单独核验；测试通过数量不代表覆盖率，也不复用 #327 的历史
百分比作为本次结果。若后续请求测量，限定 Codex 审批、诊断与启动 Timeline 相关行为，
记录实际源码、features、平台和可共享证据；全 Workspace 测试与覆盖率仍需明确请求。
