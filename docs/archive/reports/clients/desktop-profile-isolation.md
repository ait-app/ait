# Ait 与 Paseo 桌面隔离验证

日期：2026-09-27。决策见 [ADR-055](../../../decisions/clients/adr-055-desktop-profile-isolation.md)。

## 原因与修改

本机已安装 Ait 0.0.7 与 Paseo 0.9.2。检查两者的 Info.plist 和 ASAR 后确认：虽然
bundle ID 分别为 `dev.ait.desktop` 和 `sh.paseo.desktop`，Ait 启动仍显式选用
`Application Support/Paseo`，共享设置、Chromium 数据和单实例锁；两者还注册同一
`paseo` scheme。该行为在 `82bf86e` 发布提交中保留。

- 启动前统一设置 Ait 的 `userData`、`sessionData`；开发 worktree 和显式测试目录继续隔离。
- 页面来源、Agent 深链接、打包协议和共享客户端 scheme 改为 `ait`；旧 Paseo Agent 链接拒绝解析。
- 浏览器分区改为 `persist:ait-browser`，同步沙箱 preload 和相关测试。
- 打包 metadata 使用 `@ait/desktop`，更新缓存从 `@getpaseodesktop-updater` 改为
  `@aitdesktop-updater`；更新诊断使用 Ait 的 ShipIt 目录，打包资源校验同步更新。
- 新 scheme 同样经过诊断日志脱敏；移动测试链接随共享 scheme 更新。

旧 Paseo profile 不自动迁移、复制或删除。Ait 使用独立设置，浏览器登录和远程连接需重新
配置。`~/.ait-server-desktop` 中的 Ait 项目与会话不改动；本轮未操作本机应用数据。

## Test coverage

| 检查                                                                        | 结果                                                                                                                  |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Desktop：branding、Agent 导航、沙箱 preload、浏览器分区/webview、更新诊断   | 6 个文件，29 项通过                                                                                                   |
| Protocol：Agent 深链接编码、往返解析与旧 scheme 拒绝                        | 1 个文件，2 项通过                                                                                                    |
| App：诊断脱敏、resident webviews 的 Chromium 浏览器回归                     | 2 个文件，19 项通过                                                                                                   |
| 打包资源校验                                                                | 4 项通过，包含 Ait package identity 接受与 Paseo identity 拒绝                                                        |
| 真实 Electron profile 隔离                                                  | 两个进程同时持有各自单实例锁；相同页面来源下 Cookie/localStorage 不串用；Ait 重启保留自身存储；Paseo 设置文件保持原值 |
| Protocol 构建、Desktop 主进程构建、App TypeScript 检查                      | 通过                                                                                                                  |
| 变更文件格式、JavaScript/TypeScript lint、移动 shell 语法与 diff whitespace | 通过                                                                                                                  |

Electron 回归命令：先执行 `npm run build:main --workspace=@getpaseo/desktop`，再执行
`node apps/desktop/e2e/profile-isolation.e2e.mjs`。该测试只使用临时 appData，结束后清理；
不启动已安装的 Ait/Paseo，也不连接用户 Rust 服务。浏览器/Electron 回归需允许本地端口和 GUI
进程；本轮沙箱内启动失败后在获准的沙箱外执行通过。

本轮没有修改 Rust，按仓库规则不运行 Rust workspace 测试。工作区已有的其他 Rust/监听配置
修改不属于本次隔离修复。未执行移动设备测试、完整安装包构建、签名公证或系统链接注册验收；
未替换 `/Applications/Ait.app`，已安装版本需通过后续打包安装更新。
