# Ait 0.0.29 发布说明

日期：2026-10-11（Asia/Shanghai）。发布准备基于 main `b7d29d3cb01e95afadb68346c4cf4094051676fd`。
最终源码以不可变标签 `v0.0.29` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

- **OpenCode 目录与配置。** 模型目录跟随原生变化，保留模型专属选项与最终顺序；
  模型、effort、mode 切换接收响应前后的原生确认通知，避免将正常切换误判为失败。
  ACP 初始化期间协调 Ait 所有的子进程，减少并发冷启动数据库竞争，活跃会话和 prompt
  保持并发。见 [PR #288](https://github.com/ait-app/ait/pull/288)、
  [ADR-135](../../decisions/providers/adr-135-opencode-catalog-readiness.md)。
- **RPC 许可与诊断。** 已进入有界队列的普通 RPC 等待共享执行许可，保留并发上限、
  超量拒绝、顺序与停机取消语义；按每次握手公告过滤启动事件。
  SDK 日志增加有界、脱敏的请求与 schema 上下文，daemon 拒绝日志增加方法、目标与预算信息。
  见 [PR #287](https://github.com/ait-app/ait/pull/287)、
  [ADR-134](../../decisions/daemon/adr-134-admitted-rpc-job-waiting.md)。
- **桌面警告。** Native 才加载原生通知模块，移除已删除的 Plugin 路由，选择平台支持的
  动画驱动；设置页移除旧 daemon 命令提示及其翻译。
  见 [PR #287](https://github.com/ait-app/ait/pull/287)、
  [PR #286](https://github.com/ait-app/ait/pull/286)、
  [验证报告](../clients/release-0.0.28-warning-fixes-2026-10-11.md)。
- **官方服务域名。** 默认 API 改为 `https://aitapp.cn:8443/api`，恢复账号与中继主机时
  迁移精确匹配的旧官方地址，保留凭据、有效期和首选主机，自定义服务地址保持原值。
  见 [PR #284](https://github.com/ait-app/ait/pull/284)。
- **CI 流程。** PR 保留 Rust、UI、文档和格式质量门禁；两平台桌面构建及 nightly 发布
  仅在 main 执行。见 [PR #283](https://github.com/ait-app/ait/pull/283)。

## 安装与兼容性

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.29)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.29-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.29-macos-arm64.dmg`、`Ait-0.0.29-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。macOS 签名、公证及两平台成品 daemon
生命周期门禁通过后上传安装包、更新元数据、`BUILD-INFO.json` 与 `SHA256SUMS`。

**OpenCode 2.x 最低支持 2.0.26。** 更早的 2.x 在创建会话前显示版本与升级提示；
1.x 保留既有支持。Ait 不替换原生 CLI，也不改写其认证或全局配置。
启动协调仅覆盖 Ait 所有的子进程，外部原生进程不参与该协调。

官方服务迁移需要服务端域名与 Authing 配套配置；本次发布只更新 AIT 客户端。
发布准备时，新地址的 TLS 请求成功，未登录的 `/api/v1/me` 返回 HTTP 401；
没有使用真实账户测试生产 Authing 登录。
RPC 服务端修复需要对应 Host 更新 daemon，历史告警的全部原因仍需依据新诊断上下文定位。
根 `PKGBUILD` 源码配方同步为 `0.0.29-1`；移动端与 AUR 二进制包继续使用独立流程。

## 发布准备验证

版本与锁文件只更新 13 个本地 Cargo 包、根 npm 包及 6 个 npm workspace，第三方依赖
沿用发布准备基线的锁定条目。

本地验证：

- `npm ci --no-audit --no-fund`、发布版本一致性与 6 个本地 workspace 检查通过。
- `npm run test:release` 47 项、`npm run test:mobile-release` 41 项通过。
- 桌面主进程构建、桌面与移动端类型检查通过。
- 全仓 Oxfmt 检查通过，共 3280 个文件；变更文件 UI lint 无适用源文件。
  文档检查的 1 项测试及 329 份 Markdown 的本地链接验证通过。
- `cargo metadata --locked --offline`、`cargo fmt --all --check`、`bash -n PKGBUILD`
  和 `git diff --check` 通过。

PR 的适用检查、合并后的完整 main CI 与正式发布工作流全部通过，并核验下载资产的
构建来源和摘要后，才确认正式安装包发布完成。仓库既有 CI 门禁继续自动执行。

## Test coverage

**Not measured。** 用户未请求覆盖率；本次仅修改版本、锁文件和文档，没有修改
`bins/` 或 `crates/` 的 Rust 源码，按仓库规则跳过本地 Rust 测试与覆盖率测量。
功能变更的定向验证见对应 PR，历史 OpenCode 覆盖率不代表本次发布源码，测试数量也不代表
行覆盖率。若后续请求测量，按目标功能选择相关 crate 与定向测试，并提供可共享报告；
工作区覆盖率另需明确请求。
