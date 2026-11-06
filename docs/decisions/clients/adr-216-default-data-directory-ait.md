# ADR-216：默认数据目录统一为 `~/.ait`

- 状态：Accepted
- 日期：2026-11-07
- 修订：[ADR-062](adr-062-ait-runtime-paths.md) 第 4 条、[ADR-055](adr-055-desktop-profile-isolation.md) 第 6 条（默认数据目录）

## 背景

独立 daemon 默认使用 `~/.ait-server`，桌面托管 daemon 默认使用 `~/.ait-server-desktop`。
同一个 daemon 有两个默认位置，文档与排障都要分别说明。已有目录中保存着 SQLite 数据、
本地凭据和工作树，且 Git 元数据、数据库记录和 Agent 会话都引用其中的绝对路径。

## 决策

- 未通过 `--data-dir` 或 `AIT_SERVER_DATA_DIR` 显式指定时，daemon 与桌面应用都在用户主目录下
  依次查找 `.ait`、`.ait-server-desktop`、`.ait-server`，使用第一个已存在的目录。
- 三者都不存在时使用 `~/.ait`，由 daemon 首次启动时创建。
- 不移动、重命名、链接或合并已有目录；已有绝对路径保持有效。
- 显式路径优先级不变，环境变量名 `AIT_SERVER_*` 不变。开发桌面与开发 Web 的 `.tmp` 目录不变。

## 影响

- 新安装的桌面应用与独立 daemon 默认共用 `~/.ait`；两者同时以默认目录运行时，实例锁只允许一个
  daemon 启动。需要同时运行时，为其中一个显式指定数据目录。
- 同时存在 `~/.ait-server-desktop` 与 `~/.ait-server` 且没有 `~/.ait` 的机器上，独立 daemon
  默认改用 `~/.ait-server-desktop`。需要保留原目录时，显式设置 `--data-dir ~/.ait-server`。
- 之后创建的 `~/.ait` 优先于旧目录；旧目录内容不会自动迁入。
