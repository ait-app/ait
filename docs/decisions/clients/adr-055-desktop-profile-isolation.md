# ADR-055：Ait 与 Paseo 的桌面数据和链接隔离

- 状态：Accepted
- 日期：2026-09-27
- 关联：ADR-001 v4、ADR-048、ADR-053
- 替代：ADR-053 中继续使用 `paseo` 链接协议的决定
- 后续补齐：[ADR-056](adr-056-paseo-coexistence.md) 覆盖共享资源、环境变量、SSH 和移动应用 ID。

## 背景

Ait 0.0.7 改名时保留了 Paseo 的 Electron 用户目录，同时继续使用 `paseo://app`
页面来源、浏览器分区和系统链接协议。与 Paseo 共存时，两者会读取同一份设置、Cookie、
主机连接与界面缓存，并争用单实例锁和系统链接处理器。

## 决策

1. 在初始化 Chromium 和单实例锁前，将 `userData` 和 `sessionData` 一起设置到
   `<appData>/Ait`；macOS 为 `~/Library/Application Support/Ait`。
   开发 worktree 使用 `Ait-<worktree>`。显式测试应用名和用户目录覆盖继续生效。
2. Ait 注册并生成 `ait://` 链接，内置页面为 `ait://app`；不注册或接收 Paseo Agent
   深链接。共享移动客户端同步使用 `ait` scheme，移动应用 ID 不在本次桌面修复范围内。
3. 内置浏览器使用 `persist:ait-browser`。主进程、沙箱 preload 和界面使用同一分区。
   内部 IPC 名和局部 storage key 不属于跨应用资源，保留其兼容名称。
4. 打包时将 package name 覆盖为 `@ait/desktop`，使更新下载缓存独立于 Paseo；源码
   workspace 名保持不变。macOS 更新诊断读取 `dev.ait.desktop.ShipIt`。
5. 不自动复制、移动或删除共享 Paseo profile。无法可靠区分其中哪些连接、Cookie 和
   设置属于 Ait，整体迁移会再次引入串用。新 profile 首次使用默认设置，浏览器需重新登录，
   远程主机需重新添加。用户主动覆盖到同一目录时，不能保证应用隔离。
6. 独立 Rust 服务的 `~/.ait-server-desktop` 和 `AIT_SERVER_DATA_DIR` 保持原样；项目、
   Agent 和会话记录不迁移或清空。本次不改变 domain、Run、Message 或服务协议边界。

## 验证

覆盖默认、worktree、测试名和显式路径，验证旧目录保留且不会被导入；检查 Agent 链接的
生成、解析和旧协议拒绝；以临时目录中的真实 Electron 进程验证同时启动与存储隔离。
结果见[验证报告](../../reports/clients/desktop-profile-isolation.md)。
