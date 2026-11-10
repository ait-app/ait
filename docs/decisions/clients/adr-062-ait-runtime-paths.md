---
id: ADR-062
status: accepted
date: 2026-09-28
paths:
  - apps/desktop
  - bins/daemon
---

# ADR-062：Ait 运行路径与项目配置文件

- 延续：ADR-055、ADR-056、ADR-061

## 决策

1. App 不从 `.paseo/worktrees` 或任何固定路径前缀推断项目归属。工作区分组、目录建议和新 Agent 目录使用 server 的 projectPlacement / checkout 元数据；缺少元数据时保留 cwd，不猜测主仓库。
2. 项目配置默认采用 `ait.json`。仅在该文件不存在时读取 `paseo.json`；首选文件格式错误时不回退。保存校验当前有效文件的 revision，写入 `ait.json`，保留旧文件。配置存储、脚本执行和工作树配置复制使用同一组文件名常量。
3. 新工作树将未提交的旧配置复制为 `ait.json`；checkout 自带的配置优先，不能被源目录未提交的旧配置覆盖。已存在的旧文件可继续兼容读取。
4. App 原生附件新缓存目录为 `ait-native-attachments`，旧附件保留自己的绝对 storageKey。搜索识别 `.ait` / `.ait-server`，克隆临时目录使用 `.ait-clone-`。服务器主数据目录沿用既有 `AIT_SERVER_DATA_DIR`、CLI 的 `~/.ait-server` 和桌面的 `~/.ait-server-desktop`。
5. 删除不再打包的 Node CLI shim、安装入口和 passthrough；桌面启动保持 GUI / 项目路径 / agent 深链接处理。删除依赖旧 Node server、relay、插件或 CLI 的失效桌面测试启动器，生命周期脚本调用现有 Rust 启动测试。
6. 项目发行版权、作者与维护者署名为 Necokeine；上游 Paseo 及其他第三方代码的原始许可与归属声明保留。

## 兼容范围

`apps/desktop`、`packages/*` 是当前真实仓库目录；共享 `@getpaseo/*` 包名、IPC 和存储键尚未重命名。历史 ADR、上游来源目录与第三方声明保持原有名称。旧配置文件名仅用于读取兼容和对应回归测试。

## Test coverage

提交准备已在代码版本 `a933517` 运行 `RUST_TEST_THREADS=4 cargo llvm-cov --workspace --html`：workspace 91.60%（35,744 / 39,021 行），filesystem 88.14%（7,881 / 8,941 行），metadata 89.82%（6,178 / 6,878 行）。无可比基线。测量范围、跳过项、共享覆盖率摘要和未覆盖行为见[覆盖率报告](../../archive/reports/clients/ait-e2e-coverage.md)；测试执行结果见[迁移报告](../../archive/reports/clients/ait-e2e-migration.md)。
