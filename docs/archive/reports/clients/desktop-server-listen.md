# 桌面 Host 监听设置

Host 概览的现有 Daemon 卡片内分别编辑监听地址和端口，失焦或按 Enter 自动保存；
当前实际监听地址直接列在 Status 中，无独立段落、顶部说明或 Save 按钮。保存后在桌面下次
启动服务时使用。端口 0 自动分配；支持本机、LAN IPv4、IPv6 与通配地址。当前连接不会因保存
中断。配置保存在 Electron 用户数据目录的 `desktop-settings.json`，字段为
`settings.daemon.listen`，默认 `127.0.0.1:0`。

主进程继续持有子进程凭据。通配监听与连接地址分开处理；配置变化会重置旧端口缓存。
环境变量 `AIT_SERVER_LISTEN` 优先于保存值，界面会提示覆盖。协议和领域模型不变；
[ADR-054](../../../decisions/clients/adr-054-desktop-daemon-listen.md)记录网络监听和配置所有权。

## 界面风格修订验证

本次修订仅修改前端布局、自动保存交互、文案和对应测试。监听配置与 Rust 实现沿用此前结果。
输入复用现有字体设置的行布局和失焦/Enter 提交方式；异步保存期间继续编辑另一个字段时，
提交按顺序保存，避免丢失后一次修改。非法输入和写入失败在对应行显示错误。

- `npm run typecheck -w @getpaseo/app` 通过。
- `npx vitest run src/desktop/components/daemon-listen-rows.test.tsx`（在 `apps/mobile`）：5 passed，
  覆盖失焦/Enter 保存、重复提交去重、非法输入、失败重试和并发编辑顺序。
- 变更前端文件 ESLint、新增组件/测试及 E2E 的 Prettier 检查通过。
- 更新后的 `server-listen.e2e.mjs` 在真实 Electron 中通过三次启动，确认无独立标题/说明/Save、
  字段位于 Daemon 内、地址位于 Status 内，以及自动保存后的重启恢复；截图已检查。

## 首轮功能验证

- App/Desktop TypeScript 类型检查与 desktop 主进程构建通过。
- App 表单、启动登记与设置迁移回归：99 tests passed。
- Protocol IP/port 校验与通配连接地址回归：17 tests passed。
- Desktop 设置、IPC、传输与真实子进程最终合并回归：35 tests passed，无跳过。
- 设置 `AIT_SERVER_BIN=/tmp/ait-agent-interface-target/debug/daemon` 后，真实子进程回归：6 passed，
  包含 IPv4/IPv6 通配、具体 LAN IP、固定端口、重新启动读取配置、环境覆盖与认证凭据范围。
- Rust 选定 crate 覆盖率运行：133 tests passed，无跳过。
- `CARGO_TARGET_DIR=/tmp/ait-agent-interface-target cargo test --workspace`：1701 passed、0 failed、8 ignored。
  跳过项依赖真实模型凭据/付费调用、已安装原生 CLI 或外部 worker；没有额外排除测试。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、App 变更代码 ESLint 通过。
- Electron 三次真实启动的端到端验证通过，并检查了 Host 概览截图。首次执行因 Metro 冷构建超过
  60 秒等待预算失败；构建完成后重跑通过，测试的启动预算已调整为 120 秒。

Electron 端到端命令（需要本机 Expo 服务，例如 `npm run web:expo -w @getpaseo/app -- --port 8082`）：

```sh
AIT_SERVER_BIN=/path/to/server npm run test:e2e:server-listen -w @getpaseo/desktop
```

此测试使用隔离的 Electron/server 数据目录，通过真实设置表单保存，连续三次启动桌面，
检查配置文件、通配监听可连接性、固定端口及 Host 身份恢复。

## Test coverage

本次界面修订：**not applicable — no Rust behavior changed**；依照仓库规则不重跑 Rust 测试或覆盖率。
下面的数据属于前一轮监听功能实现，未作为本次界面变更的新测量结果。

测量修订：`4583883` 加本轮监听配置工作区变更，macOS aarch64，默认 features。

已完成选定 crate 测量：

```sh
cargo llvm-cov -p api -p daemon --html --output-dir /tmp/ait-listen-coverage
```

| 范围                | 已覆盖/总行数 | 行覆盖率 |
| ------------------- | ------------: | -------: |
| daemon              |       776/827 |   93.83% |
| api                 |     1094/1138 |   96.13% |
| 两个变更 crate 合计 |     1870/1965 |   95.17% |

[逐文件覆盖率摘要](../daemon/server-listen-coverage.json)是随实现提交的可审查工件；本机 HTML 在
`/tmp/ait-listen-coverage/html/index.html`。没有相同修订、相同范围的改动前基线，不能给出增量。
选定 crate 没有筛除测试；未运行 Linux/Windows。未覆盖行主要是既有异常/清理路径，需在相应平台
和故障注入环境进一步验证。

完整 workspace 测量也已完成：

```sh
cargo llvm-cov --workspace --html --output-dir /tmp/ait-listen-workspace-coverage
```

Workspace 行覆盖率为 **85.25%（57599/67563）**；两个变更 crate 的结果与上表一致。
插桩测试 1700 passed、0 failed、8 ignored；该命令默认不包含 doctest，普通 workspace 测试
另有 1 个 doctest 通过。未额外排除文件或筛选测试，8 个跳过项与普通测试相同。
[Workspace 各 crate 与逐文件统计](../workspace/server-listen-workspace-coverage.json)提供可审查工件；
本机 HTML 为 `/tmp/ait-listen-workspace-coverage/html/index.html`。没有同范围的改动前基线。
总体未覆盖行为包含既有 Provider 真实调用、平台相关进程/文件异常和防御性错误分支，
需在具备对应 CLI/凭据的平台测试和故障注入中补齐。
