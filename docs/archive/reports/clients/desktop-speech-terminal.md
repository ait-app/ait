# Desktop 语音与 Terminal 验证

日期：2026-09-28。最终提交准备范围：main `47d4b7ee`（0.0.8）加本次修改；实机 macOS ARM64。
下方 JSON artifact 记录基准完整 revision 和变更源码 SHA-256，可核对实际测量代码。

## 结果与限制

- Codex 0.157.1 实验 app-server 的 thread/realtime/start 在现有登录方式下返回
  `realtime conversation requires API key auth`。仅创建临时测试 thread；没有提交录音或执行 Agent 工具。
- 默认离线后端自动下载官方 SenseVoice/Kokoro 模型，安装到隔离的临时缓存，成功生成约 5 秒测试语音。
  两轮真实 Desktop SDK 听写均能经 Rust 连接层、离线 worker 返回中文转写，连接保持在线。
  中英文混说样例的英文部分有误识别；此验证证明功能链路，不代表语言质量基准。
- `otool -L target/debug/daemon` 仅列出 macOS 系统库；推理不依赖 Homebrew、Python、外部 CLI 或额外 dylib。
- 使用 0.0.8 安装包中的 server、独立数据目录和修改前客户端适配器，普通新终端 80×24 能创建和交互；
  首次挂载先发送 240×50 resize 时，后端返回 invalid_message，适配器将其当作 transport error，SDK 断连。
  该路径已修复，尺寸上限调整为 500 列、200 行、50,000 cells；输入失败记录错误码和尺寸，不记录输入文本。
- 新终端同样会按面板尺寸立即 resize；用户侧“仅 Terminal 显示 Host 未连接/界面闪烁”的精确触发条件
  没有拿到日志，不能据此认定已经确认用户机器上的唯一根因。
  回归覆盖正常/宽面板的新建、首次挂载、输出、输入、capture、释放、kill，以及非法 resize 后 Host 存活。
- App 现在正确声明已有 ANSI restore、输入模式回放和尺寸所有权能力。超预算的 legacy JSON snapshot
  局部失败，避免耗尽整条连接队列。连接错误与应用事件错误分开处理；真实 socket 错误仍走重连。
- 没有操作或替换用户安装的 Ait，没有发布新版本。未执行 Electron UI 手工验收或 Linux 发布构建。

## 前端闪烁路径检查

- `apps/desktop/src/main.ts` 的主窗口只在创建时 loadURL；Terminal 操作没有重新 loadURL 的分支。
  菜单 Reload / Force Reload 会显式重载，但不是新建 Terminal 的调用路径。
- `apps/desktop/src/window/compositor-watchdog/index.ts` 在 macOS 可见窗口连续三次没有动画帧
  （约 6 秒）时重启 GPU；恢复间隔至少 60 秒，未恢复前最多连续三次。此路径不同于快速连接状态闪烁。
- `apps/desktop/src/daemon/local-transport.ts` 只转发 IPC/WebSocket 事件，不自动重载 BrowserWindow。
  Rust 错误由 renderer 的 transport adapter 错误升级为 transport error 后，SDK 会重连；
  `TerminalPane` 过去在 `!client || !isConnected` 时早返回状态页，卸载终端 DOM/canvas。
- 本次除修复事件错误分类，还改为断线时保留终端渲染器、显示覆盖状态、失焦并禁止输入；
  重连不会因为这个条件分支销毁再创建终端。没有伪造在线状态。
- `apps/desktop/src/main.ts` 补充主 frame 加载、renderer 退出和无响应日志，进入 Desktop app log，
  不打印页面 URL/终端文本。已有应用诊断包含该日志，daemon.log 不包含这些界面事件。
- 扩充现有前端检查覆盖 terminal controller、renderer readiness、focus claim 与 macOS watchdog。
  TerminalPane 和 main.ts 另通过 esbuild 转译与 oxlint。实际 Electron 画面/挂载次数仍未做 UI 验收。

## 提交准备验证

本次审查将修改移到最新 main，保留 0.0.8 的 `@ait/*` 包迁移，解决 Cargo.lock 冲突，
将离线语音决策编号改为 ADR-064，并补充断线时禁止重新请求焦点、恢复连接后重新自动聚焦。

| 检查                                                                                                                              | 最终结果                                                            |
| --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `cargo test --workspace --offline`                                                                                                | 1,289 passed、0 failed、3 ignored                                   |
| `cargo build --workspace --offline`                                                                                               | 通过                                                                |
| `cargo clippy --workspace --all-targets --offline -- -D warnings`                                                                 | 通过                                                                |
| `cargo fmt --all --check`、`git diff --check`                                                                                     | 通过                                                                |
| `PASEO_TEST_DEPS=<existing-node_modules> PASEO_VALIDATION_REPORT=<report> node scripts/validate-paseo-rust-client.cjs --terminal` | 8 文件 / 76 测试、7 项隔离集成通过；0 schema 错误                   |
| `node node_modules/@typescript/native-preview/bin/tsgo.js --noEmit -p apps/desktop/tsconfig.json`                                 | 通过                                                                |
| `node node_modules/@typescript/native-preview/bin/tsgo.js --noEmit -p apps/mobile/tsconfig.json`                                  | 通过                                                                |
| `oxfmt --check`、`oxlint --deny-warnings`（本次 6 个 JS/TS/TSX 文件）                                                             | 通过，0 warning、0 error                                            |
| 最新 debug server 的真实离线 worker                                                                                               | Kokoro 生成约 5 秒 WAV，SenseVoice 返回中文转写；混说英文仍有误识别 |

3 个 ignored 用例均为仓库既有的真实 Codex/Claude 登录或模型请求测试，本次未额外跳过测试。
前端复用已有依赖并在本工作区构建 protocol/client/highlight；`npm run build:ui-deps` 的最后一步
在未修改的 `expo-two-way-audio` 包遇到 React Native 与 DOM 声明冲突，故不声称该总构建通过。
随后 Desktop/App 的独立类型检查均通过；干净依赖安装与 Linux 构建交给 PR CI。

## 前期针对性 Test execution

以下为各次针对性运行，计数不累加成覆盖率：

| 命令/检查                                                                                                                                        | 结果                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `cargo test -p voice -p terminal --lib --offline`                                                                                                | voice 55、terminal 82 通过（随后另加 snapshot 边界用例）                 |
| `cargo test -p terminal wide_desktop --offline`                                                                                                  | 新增大尺寸 snapshot 边界用例 1 通过                                      |
| `cargo test -p terminal --lib --offline connection::tests`                                                                                       | 日志修改后的连接层 20 通过                                               |
| `cargo test -p daemon --test process terminal --offline`                                                                                         | 真实 server 的终端集成 7 通过                                            |
| `cargo test -p daemon --test process voice --offline`                                                                                            | Provider HTTP 语音、Agent 和断线取消集成 1 通过                          |
| `PASEO_TEST_DEPS=<node_modules> node scripts/validate-paseo-rust-client.cjs --terminal`                                                          | 前端 76 测试通过，7 项隔离集成检查通过，无协议 schema 错误               |
| `cargo clippy -p voice -p terminal -p daemon --all-targets --offline -- -D warnings`                                                             | 通过                                                                     |
| `cargo build -p daemon --bin daemon --offline`                                                                                                   | 通过                                                                     |
| `cargo fmt --all --check`、`git diff --check`                                                                                                    | 通过                                                                     |
| `oxlint --deny-warnings apps/mobile/src/runtime/rust-daemon/{transport.ts,messages.ts,transport.test.ts} scripts/validate-paseo-rust-client.cjs` | 0 warning、0 error；另检查 terminal-pane.tsx 和 apps/desktop/src/main.ts |

前端检查使用现有依赖目录，不执行 npm install；`--terminal` 直接使用仓库内导入的 SDK，
不会要求外部 Paseo checkout。原有完整适配器验证入口仍检查 upstream pin。
可审查的终端集成结果：[JSON](desktop-terminal-validation.json)。

## Test coverage

测量命令：`cargo llvm-cov --workspace --html --offline`；随后通过
`cargo llvm-cov report --json --summary-only --output-path /tmp/ait-pr-coverage.json` 导出同一次测量。
覆盖率运行同样为 1,289 passed、3 ignored，计数与上方测试结果分开记录。

| 范围           | 行覆盖率 | 覆盖 / 总行数   |
| -------------- | -------- | --------------- |
| Rust workspace | 91.27%   | 36,250 / 39,716 |
| bins/daemon    | 94.24%   | 851 / 903       |
| terminal       | 89.66%   | 1,301 / 1,451   |
| voice          | 87.20%   | 1,655 / 1,898   |

共享 artifact：[覆盖率 JSON（含各文件、crate 计数和源码哈希）](desktop-speech-terminal-coverage.json)。
本地 HTML 另在 `target/llvm-cov/html/index.html`，此本地路径不是共享 artifact。
范围为默认 features、macOS ARM64、全工作区；没有显式文件排除，测试源码使用 cargo-llvm-cov 默认排除，
默认不 instrument doctest（当前无 doctest 用例）。没有同代码基线、同条件的可比测量，不计算变化百分点。

真实 ONNX 推理、官方下载来自前述人工功能验证，未纳入 instrumented 测量；自动化测试使用本地 archive
和 worker 替身。Linux/Windows、实际 Electron 挂载生命周期、真实麦克风和用户侧精确触发场景尚未覆盖，
后续需要 CI 与发布实机验收；不能把这些限制解释为已确认修复用户机器上的全部闪烁原因。
