# Ait 0.0.28 发布说明

日期：2026-10-11（Asia/Shanghai）。发布准备基于 main `7c62a20342e8e53fb82eb52abe748824b52d2279`。
最终源码以不可变标签 `v0.0.28` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

- **原生事件与启动订阅。** 补齐用量事件和控制事件时间戳，兼容旧主机的对应格式；
  每次握手与重连按当前主机公告协商事件，后台繁忙时等待目录和标签初始化。
  更新器诊断正确处理 ShipIt 已移走的暂存 bundle。
  见 [PR #277](https://github.com/ait-app/ait/pull/277)、
  [安装后诊断报告](../clients/release-0.0.27-installed-diagnostics-2026-10-11.md)。
- **会话切换。** 先等待释放已建立的 Timeline 订阅，再申请最新的标签集合，
  避免切换会话时订阅名额短暂耗尽。尚未完成初始化的旧订阅继续回收迟到 ID。
  见 [PR #257](https://github.com/ait-app/ait/pull/257)。
- **先显示已保存历史。** daemon 重启后，有持久历史的 Agent 先显示已保存尾页，
  再后台核对原生历史；上翻保持有界游标分页，迟到响应不覆盖已切换的会话。
  见 [PR #281](https://github.com/ait-app/ait/pull/281)、
  [ADR-133](../../decisions/providers/adr-133-cached-timeline-first.md)。
- **Changes 刷新。** 前台 Checkout 使用独立串行额度，diff 等待现有输出容量，
  同连接的小型控制响应仍可发送，修复繁忙和大 diff 累计输出时的两条断线路径。
  见 [PR #279](https://github.com/ait-app/ait/pull/279)、
  [验证报告](../workspace/changes-refresh-transport-2026-10-11.md)。
- **浏览器客户端。** 支持同源网站共享登录、持久账户会话、在线 Host、浏览器下载，
  设置内增加托管网站的管理 tab。见 [PR #280](https://github.com/ait-app/ait/pull/280)、
  [ADR-132](../../decisions/clients/adr-132-web-console-client.md)。
- **组件维护。** Agent Skills 归 Provider，Workspace 生命周期 RPC 归 metadata；
  metadata 按能力分组，保留 wire 方法契约；更新产品指南，独立执行变更文件 UI lint。
  见 [PR #276](https://github.com/ait-app/ait/pull/276)、
  [PR #278](https://github.com/ait-app/ait/pull/278)、
  [PR #259](https://github.com/ait-app/ait/pull/259)。

## 安装与兼容性

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.28)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.28-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.28-macos-arm64.dmg`、`Ait-0.0.28-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。macOS 签名、公证和两平台成品 daemon
生命周期门禁通过后上传安装包、更新元数据、`BUILD-INFO.json` 与 `SHA256SUMS`。

Changes 与服务端订阅修复需要对应 Host 更新 daemon。缓存历史读取按主机能力协商，
旧主机回退严格查询；首次原生导入与未完成的历史替换仍等待原生校验。
缓存可能暂时显示停机前的内容，后台校验补齐；没有测量实际设备的延迟改善。

托管浏览器模式需要 ait-server 配套构建、托管与同源登录；桌面 Release 不部署网站。
此模式仅提供在线服务连接，单文件下载上限 200 MiB，桌面与原生端保留自己的连接方式。
根 `PKGBUILD` 源码配方同步为 `0.0.28-1`；移动端和 AUR 二进制包继续使用独立流程。

## 发布准备验证

版本与锁文件只更新 13 个本地 Cargo 包、根 npm 包及 6 个 npm workspace，第三方依赖
沿用发布准备基线的锁定条目。

本地验证：

- `npm ci --no-audit --no-fund`、发布版本一致性与 6 个本地 workspace 检查通过。
- `npm run test:release` 46 项、`npm run test:mobile-release` 41 项通过。
- 桌面主进程构建、桌面与移动端类型检查通过。
- 全仓 Oxfmt 检查通过，共 3273 个文件；变更文件 UI lint 无适用源文件。
  文档检查的 1 项测试及 324 份 Markdown 的本地链接验证通过。
- `cargo metadata --locked --offline`、`cargo fmt --all --check`、`bash -n PKGBUILD`
  与 `git diff --check` 通过。

发布 PR、合并后的 main CI 与正式发布工作流全部通过，并核验下载资产的构建来源和摘要后，
才确认正式安装包发布完成。仓库既有 CI 门禁继续自动执行。

## Test coverage

**Not measured。** 用户未请求覆盖率；本次仅修改版本、锁文件和文档，没有修改
`bins/` 或 `crates/` 的 Rust 源码，按仓库规则跳过本地 Rust 测试与覆盖率测量。
功能变更的定向测试见对应 PR，测试数量不代表行覆盖率。若后续请求测量，按目标功能选取
相关 crate 与定向测试，并提供可共享的覆盖率报告；工作区覆盖率另需明确请求。
