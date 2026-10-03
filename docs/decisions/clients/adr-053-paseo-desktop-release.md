# ADR-053：Ait 0.0.7 桌面发布切换到 apps/desktop

- 状态：Accepted
- 日期：2026-09-27
- 关联：ADR-048、ADR-051
- 修订：链接协议和桌面数据隔离以 [ADR-055](adr-055-desktop-profile-isolation.md) 为准。
- 修订：保留旧桌面源码与独立版本的条款由 [当前架构](../../architecture/README.md) 取代。

## 背景

开发中的 Ait 桌面已经由 `apps/desktop` 承载，并使用 `apps/mobile` 导出的界面与独立 Rust
`server`。原 GitHub Release 流程仍构建 `apps/desktop`，携带 `ait-daemon` 和 `ait-worker`，
与实际开发入口不一致。

## 决策

1. 从 0.0.7 起，正式桌面发布入口为 `apps/desktop`；`apps/desktop` 保留为旧实现，退出发布和桌面 CI。
2. 沿用 Linux x86_64 与 macOS Apple Silicon arm64。产物为 AppImage、tar.gz、DMG、ZIP；
   不发布 Windows、其他架构或独立 CLI 包。
3. 只执行 `cargo build --locked --release -p daemon --bin daemon`，暂存输入先验证
   `server --version`，清理历史残留，再显式复制单个文件。成品 `resources/bin/` 必须恰好只有
   可执行的 `server`；不携带 `ait-daemon`、`ait-worker`、`ait` 或 Paseo CLI shim。
   Electron 自身的主程序、Helper 和 Chromium sandbox 仍是桌面运行时的一部分。
4. `apps/mobile` 的 Electron Web 导出随包放入 `app-dist`；Node CLI/server 包不进入 ASAR。
   macOS 签名包含内置 `server`，正式发布必须完成签名、公证和隔离启动验证。
5. 正式应用 ID 使用旧 Ait 的 `dev.ait.desktop`，产品名为 Ait；继续支持客户端使用的 `paseo`
   链接协议。不能用上游 Paseo 的 `sh.paseo.desktop` 作为 Ait 正式应用身份。
6. 发布安装包、自动更新 YAML、可用的 blockmap 与 SHA256SUMS。收集阶段核对更新文件引用、
   版本与 SHA-512；两个平台都成功后才发布。AppImage 文件名不含版本，供更新器原位替换。
7. 根 Cargo 版本、所有活跃 npm workspace 与 lockfile 同步为 0.0.7；旧桌面独立版本不再属于
   发布门禁。准备工作不自动创建标签或上传 GitHub Release。

## 数据与边界

不改变领域、Run 或 Message 边界。新 server 默认数据目录为 `~/.ait-server-desktop`，
可用 `AIT_SERVER_DATA_DIR` 覆盖。旧桌面的 `ait.sqlite3` 和 Project SQLite 数据不在本次
发布切换中自动导入；原数据保留。更新安装包不能被描述为旧数据库迁移。

## 验证与后果

发布脚本拒绝错版 server、额外 sidecar、缺失安装包、错误更新摘要或意外平台资产。
打包后的应用通过真实 server 验证自动启动、鉴权连接、重启后的重连及退出回收。
Linux 与 macOS 分别在原生 runner 执行；本地 macOS 的未公证验证包不能替代正式公证。

操作步骤见 [发布操作指南](../../operations/releasing.md)，本次结果见
[0.0.7 发布准备报告](../../reports/releases/release-0.0.7.md)。
