# Ait 0.0.20 发布说明

日期：2026-10-06（Asia/Shanghai）。本次准备基于最新 `main` 的
`422099d683398d2adcd28a8b0f0985ed39079b7d`；正式源码以合并后的不可变标签
`v0.0.20` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **统一浏览器登录。** 桌面与 Android 在支持 Authing 的在线服务上提供登录 / 注册入口，
  使用系统浏览器完成认证，再返回客户端；继续保留原 Ait 密码登录入口。
  会话保存、主机发现和逐主机同步沿用现有逻辑。见
  [ADR-086](../../decisions/clients/adr-086-authing-native-login.md)。
- **iOS 认证窗口。** 使用系统认证窗口完成统一登录，校验回调地址与 state，支持取消、超时
  和安全会话存储。新增原生模块需要重新构建移动安装包；iOS 由独立 TestFlight 流程发布。
- **账户有效期。** 登录界面显示账户有效期；账户到期与登录会话到期分别处理。
  配套服务配置、旧账号绑定和设备验收见
  [统一登录指南](../../operations/authing-client-login.md)。
- **工作区重置。** 创建时的分支名已经存在时复用该本地分支，避免分支改名失败，
  并保留此前改名的分支引用。同名分支由另一 worktree 使用时保持两端状态。
  见 [验证报告](../workspace/workspace-reset-existing-branch-pr-validation-2026-10-05.md)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.20)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.20-macos-arm64.dmg`、`Ait-0.0.20-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.20-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开。Android APK、Google Play 和 iOS TestFlight 使用
独立的手动发布流程。

统一登录入口需要中心服务提供配套接口、`0008_authing.sql` 迁移和浏览器确认页；
服务同时声明 `authing_enabled` 和 `native_login_enabled` 时才显示入口。
本次桌面发布不部署中心服务。旧密码登录仍可用，移动端真实设备认证流程需独立验收。

## 发布验证

13 个 Cargo 包、根 npm 包和 6 个 workspace 同步到 `0.0.20`；Cargo/npm 锁文件仅更新本地包版本。
本次版本与文档准备未修改 Rust 源码；按仓库规范跳过本地 Rust 测试与覆盖率重测。

在 macOS arm64 上，以 `422099d6` 为基线完成以下准备检查：

- `npm ci --no-audit --no-fund`：通过；离线缓存缺少新增原生模块后改用联网安装锁定依赖。
- `npm run verify:release -- v0.0.20`、`npm run verify:local-packages`：通过。
- `npm run test:release`：18 passed；`npm run test:mobile-release`：32 passed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `npm run check:docs`、变更清单与文档的 Oxfmt、
  `cargo metadata --locked --offline --no-deps --format-version 1`、
  `cargo fmt --all --check`、`git diff --check`：通过。
- 结构化比较确认两个锁文件仅更新本地包版本，第三方依赖未变。

最新 [main CI](https://github.com/ait-app/ait/actions/runs/37461718405) 已通过。
正式安装包由 `Release Ait` 工作流构建、签名、公证、成品启动验证并上传；
发布后核对资产、SHA-256、更新摘要和 `BUILD-INFO.json`。

## Test coverage

本次版本与文档改动没有新的行覆盖率测量。历史测量只对应各报告注明的源码、平台、
工具链和范围，不代表最终标签的重新测量。工作区重置分支复用修复的 Rust workspace
行覆盖率为 49,428/52,323（94.47%），filesystem 为 11,338/11,961（94.79%）；
完整测试与覆盖率运行各为 1,803 passed、0 failed、3 ignored。精确命令、源码提交、
历史基线、忽略项与可审查统计见
[验证报告](../workspace/workspace-reset-existing-branch-pr-validation-2026-10-05.md)。
统一登录改动的 TypeScript 行覆盖率未测量；定向测试范围与真实设备验收限制见
[统一登录指南](../../operations/authing-client-login.md)。
