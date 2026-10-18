# Ait 0.1.7 发布说明与验证

日期：2026-10-18（Asia/Shanghai）。上一正式版 `v0.1.6` 的 `BUILD-INFO.json`
记录实际源码为 `0d382334a5d5cd389903217f9dd0c438e386cad4`。比较到 main
`d1b42d70306afc7e46c67b4bc4383ec0e8edfcae`，纳入 5 个已合并 PR，准备补丁版 `0.1.7`。
版本与 changelog 准备、覆盖率测量提交为 `4052f44cfb93d19c8a4886bc04e0b3345d93fae2`；
后续仅增加本报告、覆盖率证据与索引。最终交付源码、工作流与运行链接以正式附件
`BUILD-INFO.json` 和 Release Notes 为准。

## 更新内容

- **实时同步可连接 Host。** 客户端使用独立账户状态通道接收上线、下线与完整快照，
  上线事件先于快照到达时也能导入并连接 Host；状态断线保留已有业务连接。
  见 [PR #349](https://github.com/ait-app/ait/pull/349)。
- **协商后复用持久 Relay 数据通道。** 支持该能力的中心与 Host 复用一条 WSS，
  每次访问使用独立 Yamux stream；保持原控制通道与旧中心的独立数据连接路径。
  见 [PR #351](https://github.com/ait-app/ait/pull/351)。
- **前台恢复时及时检测停滞连接。** 恢复时重试已等待至少 3 秒的握手，验证账户状态通道存活，
  合并重复恢复请求，避免旧连接回调干扰新连接；保留正常退避。
  见 [PR #352](https://github.com/ait-app/ait/pull/352)。
- **过滤离线 Host 与无效中继探测。** 普通 Host 选择器和首页排除不可用的纯中继 Host，
  设置仍保留已有记录；直连 Host 保持可用，Host 重新上线后恢复连接。
  见 [PR #353](https://github.com/ait-app/ait/pull/353)。
- **在定时任务列表暂停与恢复。** 已有 Agent 任务和新 Agent 任务均提供直接按钮与菜单操作，
  请求处理中防止重复操作，结束或目标不可用的任务不显示直接按钮。
  见 [PR #354](https://github.com/ait-app/ait/pull/354)。

## 兼容性与限制

账户发现需要中心支持 `/v1/relay/channel`、`ait-channel-v1` 与可路由 Host 事件；
不回退到旧 control 订阅或 HTTP 轮询。服务端配套变更见
[状态通道 PR](https://cnb.cool/ait-app/ait-server/-/pulls/4)。
持久数据通道则按能力协商启用，未声明能力的旧中心继续使用原有路径；配套中心能力见
[数据隧道 PR](https://cnb.cool/ait-app/ait-server/-/pulls/5)。本次桌面发布不等同于服务端生产部署。

3 秒为客户端前台存活检查期限，不是端到端重连耗时承诺。单条 WSS 仍有 TCP 队头阻塞；
实体移动设备后台恢复、弱网和公网 CDN 的延迟分位数未测量。

桌面标签构建 Linux x86_64 与 macOS 13+ Apple Silicon 安装包和更新元数据。
Android APK、Google Play、TestFlight 与 AUR 按各自流程独立发布。

## 发布准备与验证

同步 15 个 Cargo workspace 包、根 npm 包和 6 个 npm workspace、两份锁文件及根
`PKGBUILD` 为 `0.1.7`。结构化比对确认第三方依赖记录相对准备基线不变；npm 锁文件格式保留。
本次版本准备不新增产品代码变更。

本地验证通过：

- `npm ci --no-audit --no-fund`、`npm run verify:release -- v0.1.7` 和本地 workspace 链接检查。
- 发布脚本 53 项、移动发布脚本 41 项测试。
- `npm run build:desktop-main` 与桌面、移动端类型检查。
- SDK 账户通道、会话与 daemon client 三个文件 270 项定向测试。
- 移动端及共享界面 15 个文件 301 项定向测试，涵盖账户状态、Host 发现、导入及任务暂停恢复。
- 桌面账户通道 9 项测试，包含本机真实 WebSocket 心跳与恢复。
- `cargo clippy --locked --offline -p relay --all-targets -- -D warnings` 与 Rustfmt。

本机 Node 26.11.1 首次运行移动端测试时，原生 Web Storage 与 jsdom 冲突导致 5 项初始化失败。
设置 `NODE_OPTIONS=--no-experimental-webstorage` 后完整重跑同一组 15 个文件，301 项全部通过，
没有降低并行度或修改产品源码；正式 GitHub 工作流使用 Node 24。

适用的发布准备 PR/main CI、两平台正式构建、macOS Developer ID 签名、公证及成品 daemon
生命周期门禁全部通过后，核对完整附件的 SHA-256、更新元数据 SHA-512、标签与构建来源，
将实测证据写入 Release Notes，并按[发布指南](../../operations/releasing.md#自动完成正式发布)
自动正式发布。随后校验 COS 镜像并更新、验证官网；各步骤以实际执行记录为准。

## Test coverage

本次实际测量 **relay crate、默认 features**，macOS arm64 上行覆盖率为
**90.7060%（1015/1119）**。这不是整个 Rust workspace 或整个产品覆盖率。
测量提交为 `4052f44cfb93d19c8a4886bc04e0b3345d93fae2`。
共享[JSON 证据](release-0.1.7-coverage.json)包含逐文件覆盖行数、LCOV 零执行行及
relay、model、domain 与 Cargo 构建输入的 137 项 SHA-256 指纹，用于核对最终发布源码等价性。

测试执行结果单列：**46 通过、0 失败、0 跳过**，使用 nextest 默认并行度。
无额外文件排除，沿用 cargo-llvm-cov 默认测试源码过滤；未运行整个 Rust workspace 测试。
JavaScript/TypeScript 行覆盖率未测，前述通过数量不能替代覆盖率。

精确命令：

```sh
export CARGO_TARGET_DIR=target/release-0.1.7-coverage
cargo llvm-cov clean --workspace --locked --offline
cargo llvm-cov nextest -p relay --locked --offline --html --status-level all
cargo llvm-cov report -p relay --json --output-path /tmp/ait-release-0.1.7-coverage-raw.json
cargo llvm-cov report -p relay --lcov --output-path /tmp/ait-release-0.1.7-coverage.lcov
```

`clean --workspace` 只清理本次独立目标目录的 instrumentation 文件，不执行全工作区测试。
平台为 macOS 27.0.1 arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、nextest 0.9.148。
HTML 位于 `target/release-0.1.7-coverage/llvm-cov/html/index.html`；可共享结果以仓库 JSON 为准。

主要未覆盖行为包括隧道单次访问桥接的错误与取消分支、传输失败与关闭分支，以及本地 daemon/RPC
异常响应；后续修改这些路径时应补充定向故障注入。单次访问桥接文件为 71.5909%（63/88）。
Linux、Windows、外部中心、实体设备和 doctest 不在本次测量内。上一版测量了其他 crate，
没有可比 relay 基线，不计算百分点变化。
