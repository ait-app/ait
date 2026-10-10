# 仓库审计与修复：2026-10-10

审计起点：`2ce787fe686a2a8cb96a6bfba5e3c4dd5e3d2c2d`，提交前合并到 main `b697d6749dee2d9d2a89dd16502e06f7fea9acfc`（0.0.26 之后）。
平台为 macOS 27.0.1 arm64，
Rust 1.98.1，Node.js 26.10.0。审计覆盖 Rust daemon 与 crates、Electron 主进程、共享 Web/移动界面、
仓库内 SDK、CI/发布工作流与文档。上一次审计见[2026-10-03 仓库审计](repository-audit-2026-10-03.md)。

## 已修复的问题

### Desktop 主进程

| 问题                                  | 触发与影响                                                                                     | 修复与回归                                                                                                    |
| ------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| 「在文件管理器中打开」可启动任意文件  | `paseo:editor:openTarget` 只检查路径存在，`shell.openPath` 会直接执行 `.command`/`.app` 等     | workspace 路径必须是目录；macOS 应用/安装包 bundle 改为在 Finder 中定位；`registry.test.ts` 覆盖文件与 bundle |
| 回复中的目录链接可启动 `.app`         | macOS bundle 是目录，`openDirectoryLink` 对 Agent 输出中的链接调用 `shell.openPath` 会直接运行 | bundle 改为 `showItemInFolder`；`launchable-bundle.test.ts` 与 macOS 下的 `file-opener.test.ts` 覆盖          |
| 预览可执行类型清单缺少 macOS 启动器   | `.terminal`、`.webloc`、`.pkg`、`.dmg`、`.workflow` 等无确认直接打开                           | 补齐扩展名，沿用既有确认对话框                                                                                |
| SSH 隧道 EPIPE 使主进程崩溃           | ssh 退出时 WebSocket 仍在写入，`child.stdin` 的 `error` 无监听而成为未捕获异常                 | 为 ssh stdin/stdout 增加错误处理并关闭隧道                                                                    |
| 退出时等待 daemon 可能永久挂起        | 终止以 `close` 为准，持有 stderr 的子孙进程会阻止事件触发，`app.exit(0)` 无法到达              | 以 `exit` 结束等待，SIGKILL 后另有 5 秒上限                                                                   |
| `daemon.log` 无限增长且整文件同步读取 | 每次查看日志读取整个文件，只为展示最后 100 行                                                  | `tailFile` 仅读取末尾 512 KiB；daemon 启动时超过 10 MiB 轮转为 `daemon.log.1`；两项新回归                     |
| 非常规扩展名导致附件失败              | `notes.backup_old`、`report.2024-01` 等未通过扩展名正则时直接抛错                              | 托管副本回退为 `.bin`，内容与文件名不变；`attachments.test.ts` 覆盖                                           |
| 浏览器自动化日志级别为数字            | Electron 44 的位置参数 `level` 是已弃用的 0–3，`browser_logs` 返回 `"2"` 而非 `"warning"`      | 改用 details 对象的 `level`/`lineNumber`，测试替身同步为真实事件形状                                          |
| 通知点击在原窗口关闭后抛错            | 对已销毁的 `WebContents` 调用 `fromWebContents` 抛出 "Object has been destroyed"               | 先检查 `isDestroyed()`，再回退到任意现存窗口                                                                  |
| 启动期深链失败导致整个应用退出        | 首窗口打开后，排队的 Agent 深链失败仍使 bootstrap reject，进而 `process.exit(1)`               | 深链失败只记录日志；`activate` 处理器在首个 await 前注册                                                      |
| 畸形 `%` 转义使协议处理器抛错         | `ait://app/%E0%A4%A` 在 `decodeURIComponent` 抛出 `URIError`                                   | 返回 400                                                                                                      |
| `ait:invoke` 经原型链查找命令         | `command = "constructor"` 会解析到 `Object`                                                    | 使用 `Object.hasOwn`                                                                                          |

### 共享界面与 SDK

| 问题                                 | 触发与影响                                                                                                          | 修复与回归                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| 附件扩展名取自完整路径               | `/Users/me/my.project/Makefile` 得到 `.project/makefile`，随后桌面端拒绝该扩展名                                    | 只从文件名取扩展名；`file-types.test.ts` 覆盖 POSIX 与 Windows 带点目录        |
| Windows 文件选择器使用完整路径作名称 | `split("/").pop()` 总有值，`??` 回退从不执行                                                                        | 复用 `getFileNameFromPath`                                                     |
| 切换连接后心跳停止                   | 同一 server 由 LAN 切换到 relay 时生成新 `DaemonClient`，tracker 仍检查旧客户端的连接状态                           | tracker 随 `client` 重建                                                       |
| 账户服务非 JSON 错误丢失状态码       | 网关返回 HTML 404/502 时 `response.json()` 抛 `SyntaxError`，状态码丢失，401 登出与「服务不支持统一登录」提示均失效 | 解析失败时按空对象处理并保留状态码；在 ADR-123 的不支持服务用例中增加 HTML 404 |
| `useIsRecording` 每次渲染重新订阅    | 每次渲染生成新的 `subscribe`，`useSyncExternalStore` 反复注销与注册原生监听                                         | 订阅与快照函数提升到模块作用域                                                 |

### Rust

| 问题                                      | 触发与影响                                                                    | 修复与回归                                                                                           |
| ----------------------------------------- | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| 终端活动锁中毒后级联 panic                | 任一持锁线程 panic 后，所有 `expect("terminal activity lock")` 调用继续 panic | 集中为 `lock()`，以 `PoisonError::into_inner` 恢复（任一存储值都是有效投影）；新增中毒后继续工作回归 |
| `unsafe_code` 为 `deny` 而规范写 `forbid` | `deny` 可被局部 `#[allow(unsafe_code)]` 放开                                  | 改为 `forbid`；代码中无 unsafe，workspace clippy 通过                                                |
| 常量生产函数非 `const`                    | `default_limit()` 只返回 20                                                   | 改为 `const fn`（serde 需要函数路径）                                                                |

### CI 与发布

| 问题                                  | 影响                                                                                 | 修复                                                                          |
| ------------------------------------- | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| `changes` 失败时 nightly 仍可发布     | rust/ui 被跳过，发布条件把 `skipped` 视为通过                                        | `nightly-publish` 依赖并要求 `changes` 成功；`nightly-workflow.test.mjs` 覆盖 |
| 发布失败后仍删除 nightly 产物         | 单平台失败重跑时另一平台产物已删除，资产校验失败                                     | 仅在发布成功后清理                                                            |
| 路径过滤遗漏测试输入                  | `scripts/fixtures/**` 被 Rust 测试 `include_str!`；provider fixture 被协议测试读取   | 分别加入 rust/ui 过滤；ui 过滤补充 `cancel-merged-pr-ci.yml`                  |
| Rust 检查未锁定依赖                   | 过期 `Cargo.lock` 可通过 PR CI，只在发布时失败                                       | clippy/test 使用 `--locked`，增加 `Swatinem/rust-cache`                       |
| 多项测试与检查未在 CI 运行            | SDK 与高亮包测试、ADR-094 方法名校验及其 Python 测试、`app-identity` 测试、oxlint    | 加入 CI 或对应 npm 脚本；本地均通过                                           |
| 长任务缺少超时、PR 桌面构建不可取消   | 默认 6 小时；同一 PR 新推送不取消 90 分钟的旧构建                                    | rust/ui/publish/release 增加超时；desktop 增加按 PR 与平台分组的 concurrency  |
| iOS 发布工作流使用 v4 actions         | Node 20 运行时，且 checkout 保留凭据                                                 | 升级到 v5 并设置 `persist-credentials: false`                                 |
| `ios-testflight-release.mjs` 入口判断 | 路径含空格或非 ASCII 时 `main()` 不执行且以 0 退出                                   | 改用仓库其他脚本的 `pathToFileURL` 判断                                       |
| 文档链接检查不含 `crates`/`bins`      | `crates/persistence/README.md` 中的 ADR 链接不受检查                                 | 加入扫描目录                                                                  |
| 危险的模板脚本                        | `reset-project` 可删除 `apps/mobile/scripts`；`deploy:web` 指向上游 `paseo-app` 项目 | 删除脚本与入口                                                                |

### 文档

- README 与当前架构仍称 iOS 无账户登录、引用已不存在的 UI 文案；改为当前入口并链接 ADR-083/084/086，
  自建服务说明移入[移动端与 Web](../../../apps/mobile/README.md)；Workspace 表补充 `persistence`、`relay`。
- [ADR 分类索引](../../decisions/README.md)按目录生成：修正把 ADR-112 标为 ADR-118 的条目，ADR-099 归入
  Provider，各类按编号排序，并说明 ADR-001–024 已删除及 074/082/083/084 的跨类重号。
- [报告索引](../README.md)补齐 19 份未登记报告，顶部未分类条目归入对应类别，发布列表按版本排序。
- `docs/README.md` 统一 ADR 列表顺序与重号标注，最新发布说明与规范条目归位。
- ADR-038 标注被 ADR-111 取代；ADR-072、ADR-109 增加后续修订说明；ADR-063 修正自指的旧命令别名。
- Rust 规范删除不适用的 polars/maturin 条目，凭据规则与 daemon 不加载 `.env` 的行为一致。
- Apple 构建文档改用 `Ait.app`/`Ait.xcarchive`；daemon 手册补充 OpenCode 与 Antigravity 程序路径变量。

## 未修复与后续

- `keepRunningAfterQuit` 设置被保存但退出流程从不读取。保留 daemon 需要 detached 启动与日志重定向，
  属产品行为变更，留待单独决策。
- `copy_attachment_file` 接受任意源路径，可经托管目录间接读取任意可读文件。修复需要主进程签发
  选择/拖放路径的许可，涉及桌面拖放流程，留待单独变更。
- `useDesktopAppUpdater` 每个调用方各自创建 store 并检查更新，启动时可能并发 2–3 次检查。
- `apps/mobile` 约 400 个单元测试文件中，CI 只运行选定子集。本地在 Node.js 26 下
  `src/composer/draft/input-draft.live.test.tsx` 因 Node 内置 `localStorage` 覆盖 jsdom 而失败
  （修改前同样失败，`NODE_OPTIONS=--no-experimental-webstorage` 下通过）；CI 使用 Node 24 不受影响。
- `provider::service::agent_execution::routing` 在 `spawn_blocking` 中持有 `template` 锁执行
  `block_on(poll_generated_titles)` 与 `close_all`，期间其他阻塞线程上的规划会等待。重构需要调整执行状态所有权。
- `npx oxfmt --check .` 报告 123 个既有文件格式不一致，未加入 CI；可单独提交一次全仓格式化后再启用。
- AUR `packaging/aur/ait-bin` 仍为 0.0.20，按发布指南在下一次桌面发布后更新。

## 测试与静态检查

| 检查                    | 命令/范围                                                                                                        | 结果                                 |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| Rust 格式               | `cargo fmt --all --check`                                                                                        | passed                               |
| Rust lint               | `cargo clippy --locked --workspace --all-targets -- -D warnings`（含 `unsafe_code = "forbid"`）                  | passed                               |
| Rust 测试               | `cargo nextest run --locked --workspace`；`cargo test --locked --workspace --doc`                                | 2,044 passed，16 skipped；无 doctest |
| ADR-094 方法名          | `python3 scripts/check-paseo-client-methods.py` 及两项 Python 测试                                               | 172 个方法对齐；5 passed             |
| Desktop                 | `npm test --workspace=@ait/desktop`                                                                              | 421 passed，13 skipped               |
| SDK / 高亮 / 协议       | `npm run test:sdk`、`npm run test --workspace=@ait/highlight`、`npm run test --workspace=@ait/protocol`          | 275、102、794 passed                 |
| Mobile（CI 子集及改动） | CI 选定文件加 `src/attachments`、`src/hooks`                                                                     | 930 passed，1 skipped                |
| 类型检查                | `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`、`npm run build:desktop-main`               | passed                               |
| Lint                    | `npx oxlint`（全仓 0 errors）；改动的 mobile 文件运行 `eslint`                                                   | passed                               |
| 发布脚本                | `npm run test:release`、`npm run test:mobile-release`、`npm run verify:release`、`npm run verify:local-packages` | 45、41 passed；版本与本地包一致      |
| 文档                    | `npm run check:docs`（含 `crates`、`bins`）、改动文件 `oxfmt --check`                                            | 309 份 Markdown 链接有效             |

## Test coverage

Workspace 行覆盖率 **94.6687%（56539 / 59723）**，相对可比基线 **+0.0010 个百分点**。
改动的 Rust 文件 `crates/terminal/src/activity.rs` **100.0000%（121 / 121）**，
`crates/provider/src/protocol/agent.rs` **100.0000%（8 / 8）**；terminal **96.5833%（1583 / 1639）**，
provider **94.1934%（27058 / 28726）**。

命令：`cargo llvm-cov nextest --locked --workspace --html`，随后 `cargo llvm-cov report --json --summary-only`。
范围：基于 main `b697d674` 的全部 13 个 workspace package、默认 features、macOS arm64、Rust 1.98.1，
无额外文件排除；16 项原生 Provider 用例按默认配置跳过，doctest 不在测量范围，Linux/Windows 未纳入本地测量。
测试执行：2,044 passed，16 skipped（基线 2,043 passed，少本次新增的锁中毒回归）。

可比基线在同一工作树、同一命令与平台下，将三个改动的 Rust 文件恢复到 `b697d674` 后测得：
94.6677%（56545 / 59730）。terminal 的 −0.0145 pp 来自合并五处加锁调用后的行数变化；
filesystem 的 +0.0082 pp 未改动源码，属于时序相关测试在两次运行间的差异。
逐 crate 计数与改动文件覆盖率见[共享证据](repository-audit-coverage-2026-10-10.json)；
本地 HTML 为 `target/llvm-cov/html/index.html`。TypeScript 覆盖率未测量。

| Package     | 行覆盖率                  | 相对基线   |
| ----------- | ------------------------- | ---------- |
| api         | 94.5455%（2132 / 2255）   | +0.0000 pp |
| browser     | 97.5443%（715 / 733）     | +0.0000 pp |
| daemon      | 94.7520%（1318 / 1391）   | +0.0000 pp |
| domain      | 100.0000%（549 / 549）    | +0.0000 pp |
| filesystem  | 94.8649%（11583 / 12210） | +0.0082 pp |
| metadata    | 94.1562%（5462 / 5801）   | +0.0000 pp |
| model       | 96.2536%（1336 / 1388）   | +0.0000 pp |
| persistence | 95.8734%（1696 / 1769）   | +0.0000 pp |
| provider    | 94.1934%（27058 / 28726） | +0.0000 pp |
| relay       | 91.8489%（462 / 503）     | +0.0000 pp |
| schedule    | 97.6131%（777 / 796）     | +0.0000 pp |
| terminal    | 96.5833%（1583 / 1639）   | −0.0145 pp |
| voice       | 95.1605%（1868 / 1963）   | +0.0000 pp |
