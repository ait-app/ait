# Ait 0.1.5 Draft 发布说明

日期：2026-10-16（Asia/Shanghai）。每日发布将修复计入发布条件；上一正式版
`v0.1.4` 的 `BUILD-INFO.json` 经上传摘要核对，实际交付源码为
`e3c28c9c6b7f21abb2d09027999ab3137ff8151a`。本次比较到 main
`32d0b196476b5192a27ed8eb9f397025cb8ffdb9`，包含 6 项已合并修复，准备补丁版本 `0.1.5`。
当前没有其他待 review 草稿。准备中额外修复了配置保存版本戳精度问题，
最终生产源码与覆盖率测量提交为 `12e9a2ecb407b9695380aec2fccb853b7743bd81`。

## 更新内容

- **远程 Workspace 浏览器回环转发。** 在线服务中继 Host 的回环请求访问远程 daemon
  所在机器，不再误访问客户端电脑；保持原始 URL，支持 HTTP(S)／WebSocket 字节流、
  半关闭和断线后重新连接。每个 Host 使用隔离的持久浏览器 profile。
  见 [PR #337](https://github.com/ait-app/ait/pull/337) 与
  [ADR-152](../../decisions/clients/adr-152-remote-workspace-browser-loopback.md)。
- **OpenCode ACP 冷启动。** 版本查询、启动 gate 与 initialize 共用 120 秒截止时间，
  首次 session/new 和只读目录查询各允许 120 秒，OpenCode catalog 总预算为 5 分钟。
  普通控制请求仍为 30 秒，其他 Provider 不变；失败不重放请求。
  见 [PR #338](https://github.com/ait-app/ait/pull/338) 与
  [ADR-153](../../decisions/providers/adr-153-opencode-acp-cold-start-budget.md)。
- **回退菜单响应。** 选择回退后立即关闭菜单，后台等待原生请求和时间线替换；回退按钮
  显示加载并禁用，页面和输入框继续可用，保留新输入的草稿。
  见 [PR #339](https://github.com/ait-app/ait/pull/339)。
- **Codex 多文件编辑。** 按文件展示稳定卡片、有效文件路径和对应 diff，实时流和历史
  共用投影；旧缓存中的 Multiple files 卡片保留 diff，但隐藏无效的打开文件按钮。
  见 [PR #340](https://github.com/ait-app/ait/pull/340)。
- **菜单高亮。** 指针与键盘共用一个高亮，响应精细／粗指针能力变化，禁用项不接收
  指针触发的焦点。见 [PR #306](https://github.com/ait-app/ait/pull/306)。
- **Android 系统返回。** 面板展开时优先关闭面板并消费返回；中心页面的系统边缘手势
  进入相应面板，普通返回交给导航，弹层保持优先处理。
  见 [PR #336](https://github.com/ait-app/ait/pull/336) 与
  [ADR-151](../../decisions/clients/adr-151-android-panel-edge-gestures.md)。

- **配置保存冲突。** 保留 JSON 请求中的小数毫秒版本戳，避免文件未改动时被误判为
  `stale_project_config`；仍执行原有版本比对，保护真实并发修改。新增回归固定复现
  `1792160783969.9307` 被旧解析器读成 `1792160783969.931` 的情况。

## 安装与 review 范围

桌面附件预期共 10 个：Linux x86_64 AppImage／tar.gz、macOS 13+ Apple Silicon
DMG／ZIP、两份 blockmap、两份更新元数据、`BUILD-INFO.json` 和 `SHA256SUMS`。
正式 macOS App 必须通过 Developer ID 签名、公证与成品 daemon 生命周期门禁；
Nightly 使用 ad-hoc 签名且未公证。

远程浏览器转发要求客户端和远程 daemon 同时升级；旧 daemon 不支持转发时不会回退到
客户端回环。仅在线服务中继 Host 接入新路由，直接 TCP／SSH 和本机浏览器保持既有路由。
远程 profile 的非回环地址直接连接，不继承系统 HTTP 代理规则。每个连接最多 16 条流，
分片最多 64 KiB；TLS 校验由 Chromium 完成。Linux／Windows 的完整浏览器路径未在本机验证。

Android 修复包含原生模块，需重新构建安装包，不能仅靠 JavaScript 更新交付。
Android 10–13 的边缘方向为兼容来源；旧设备和工作区文件面板仍需真机验收。
本次未新增 Kotlin、Android lint 或真机验证；PR 中的历史 Pixel 验收不作为本次发布验证。
桌面标签不触发 Android APK、Google Play、TestFlight 或 AUR 发布。

按[发布指南](https://github.com/ait-app/ait/blob/fe5d25fe6d0b320d77e9fe523be26c7f3ac4a5c5/docs/operations/releasing.md#draft-后必须停下等待人工-review)完成 CI 和
附件校验后保持 Draft，公开 Latest 继续为 `v0.1.4`，等待用户 review。
最终构建源码、工作流与运行链接以附件 `BUILD-INFO.json` 为准。

## 发布准备与门禁

Rust workspace 与 15 个本地 Cargo 包、根 npm 包和 6 个 npm workspace、锁文件和根
`PKGBUILD` 同步为 `0.1.5`。结构化锁文件比对确认相对准备基线的第三方记录未变；
基线已有 browser 对 base64／tokio-util 的依赖正常保留。
本次准备同步版本、changelog 和报告；额外启用 workspace serde_json 的
`float_roundtrip`，增加 metadata 协议边界回归，并补齐离线 E2E 的 Provider 隔离默认值。
不改客户端生产源码、CI 或 AGENTS.md，第三方包版本和锁记录保持不变。
[上游实现说明](https://github.com/serde-rs/json/blob/v1.0.151/Cargo.toml#L65-L71)
指出精确解析浮点数约需原来的两倍解析成本；该成本只针对浮点数解析，未测整个应用的性能差异。

本地验证已通过：

- `npm ci --no-audit --no-fund` 安装 2288 个锁定包，版本与本地 workspace 链接验证通过。
- `npm run test:release` 53 项、`npm run test:mobile-release` 41 项通过。
- `npm run build:desktop-main`、桌面与移动端类型检查通过。
- 桌面 7 个相关文件 54 项、移动端 8 个相关文件 111 项定向回归通过。
- 以独立 target 构建本版 daemon，供浏览器转发与回退响应端到端回归使用。
- Electron 远程浏览器 E2E 通过：经认证 Rust relay 的回环 HTTP、200 KB 请求体、
  重连和 HTTP(S)／WebSocket 代理选择。
- Codex 回退响应原始 E2E 通过：请求等待期间菜单立即关闭、按钮禁用，输入框继续可用，
  保留草稿。初次本机失败来自测试夹具遗漏其他已安装 Provider 的隔离，目录发现拖慢初始化；
  给 OpenCode、DeepSeek Harness 和 Antigravity 设置不可执行的默认路径后转绿，
  不增加超时、不删断言，仍允许用例通过显式 environment 覆盖。

额外尝试的旧 `agent-message-rewind.spec.ts` 3 项在创建 `mock` Provider 时失败；
该旧夹具不被当前 Rust daemon 支持，未执行到回退行为，不视为通过，也不作为本次门禁。
当前 Codex fixture 的回退响应回归已单独通过；本次未迁移整套遗留 mock E2E。

重建 daemon 后，回退响应、认证、浏览器目录、离线 Codex 回合与配置迁移共 5 项 E2E
全部通过；Electron 中继转发 E2E 再次通过。
Rust 配置协议 6 项定向测试、三个相关 crate 的 all-targets clippy、格式和类型检查通过。
其他 API／daemon 集成行为由适用的 PR／main CI 验证；本地 Rust 测量只运行下述三个 crate。发布构建完成后在 Draft 说明中记录两平台、
签名、公证、成品启动、附件摘要及更新元数据校验，不提前宣称安装包门禁通过。

## Test coverage

本次实际测量 **browser、provider 与 metadata 三个 Rust crate、默认 features**
（包含 workspace serde_json `float_roundtrip`），合计
**88.6327%（35820/40414）行覆盖率**；不是全 workspace 或整个产品覆盖率。
测量提交为 `12e9a2ecb407b9695380aec2fccb853b7743bd81`。
后续发布准备只增加报告与证据，
[共享 JSON](release-0.1.5-coverage.json)记录全部 Rust 源码、
manifest、锁文件与 fixture 指纹，用于核对最终发布源码等价性；同时提供逐文件计数、
LCOV 零执行行和忽略用例原因。

| Crate    | 覆盖行／总行 | 行覆盖率 |
| -------- | ------------ | -------- |
| browser  | 817/905      | 90.2762% |
| provider | 29429/32137  | 91.5736% |
| metadata | 5574/7372    | 75.6104% |

测试结果单列：**1185 通过、0 失败、31 项既有忽略**；维持 nextest 默认并行度。
忽略项均来自所选 Provider crate 的原生或认证环境要求，名称与原因见共享 JSON。
没有新增忽略项、额外文件排除或覆盖率豁免，沿用 cargo-llvm-cov 默认测试源码过滤。
JavaScript／TypeScript 只运行定向回归，未测行覆盖率；未运行全 Rust workspace 测试或覆盖率。

测量平台为 macOS 27.0.1 arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、nextest 0.9.148。
Linux／Windows 条件分支、外部 Provider CLI、Python／shell fixture、doctest、Kotlin／原生
移动端不属于本次 Rust instrumentation 范围。上一版 0.1.4 测量包含全 workspace 的跨 crate
和集成测试贡献，与本次范围不一致；没有可比定向基线，不计算百分点变化。

精确命令如下。使用新建的独立构建目录；clean 清理旧 instrumented 产物，不执行测试。
实际测试和报告均只选择 browser／provider／metadata，没有借此运行整个 workspace。

```sh
export CARGO_TARGET_DIR="$PWD/target/release-0.1.5-coverage"
cargo llvm-cov clean --workspace --locked --offline
cargo llvm-cov nextest -p browser -p provider -p metadata --locked --offline --html --status-level all
cargo llvm-cov report -p browser -p provider -p metadata --json --output-path /tmp/ait-release-0.1.5-coverage-raw.json
cargo llvm-cov report -p browser -p provider -p metadata --lcov --output-path /tmp/ait-release-0.1.5-coverage.lcov
```

HTML 在 `target/release-0.1.5-coverage/llvm-cov/html/index.html`；共享 JSON 提供不依赖本机
路径的计数和定位。百分比以 LLVM JSON summary 为准，LCOV `DA=0` 仅用于定位。
重要未覆盖行为包括 tunnel 的超时／取消／断线异常，以及原生 Provider 进程与协议错误、
历史投影的异常分支，以及 metadata RPC 的项目／Workspace 生命周期路由和失败返回。
metadata 的 crate 覆盖率为 75.6104%，未借用 API／daemon 集成测试的覆盖贡献；
本次涉及的配置协议源码单独为 100%（153/153），不是整个 metadata 的覆盖率。
后续修改未覆盖路径时应补故障注入；其他平台和真实认证 Provider 还需在相应隔离环境中验证。
