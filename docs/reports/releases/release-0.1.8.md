# Ait 0.1.8 发布说明与验证

日期：2026-10-18（Asia/Shanghai）。上一正式版 `v0.1.7` 的 `BUILD-INFO.json`
记录实际源码为 `a9551f16d6c5e3d046bc44d92e1ba40768127bb6`。比较到 main
`33cafc31ef106a081d77d4a5a8c7938b29084912`，纳入 7 个已合并 PR，准备补丁版 `0.1.8`。
版本与 changelog 准备、覆盖率测量提交为 `5cf0dd2451e652c9b96fbcc4d12b706c504cf8d9`；
后续仅增加本报告、覆盖率证据与文档索引。最终交付源码、工作流与运行链接以正式附件
`BUILD-INFO.json` 和 Release Notes 为准。

## 更新内容

- **Pi 原生 RPC Provider。** 支持 Pi 1.2+ 的模型发现、创建与恢复原生会话、图片附件、
  流式文本与推理、工具进度与用量快照；等待 `agent_settled` 后结束一轮，保留 Pi 原生配置和认证。
  见 [PR #357](https://github.com/ait-app/ait/pull/357)。
- **整图发送与跨设备图片预览。** 每张图片保留为一个完整模型输入，预算内使用原始字节，
  超预算时整图编码与等比缩放；失败保留草稿。Codex 实时与历史消息共享图片来源投影，
  其他设备及重启后可通过鉴权文件 RPC 获取预览。
  见 [PR #360](https://github.com/ait-app/ait/pull/360)。
- **Claude 恢复与 Codex 身份。** Claude 按会话启动目录恢复，避免后续 shell 切换目录后取消再发消息持续失败；
  Codex app-server 客户端标识改为 `ait`、显示名 `Ait`。
  见 [PR #361](https://github.com/ait-app/ait/pull/361)、[PR #359](https://github.com/ait-app/ait/pull/359)。
- **桌面拖动与键盘布局。** 将窗口拖动排除区域限制在窗口控件与浮层，避免滚动后的聊天控件阻挡标题栏；
  为居中新建表单的输入区域设置最小容量，避免较高键盘下文字覆盖按钮。
  见 [PR #356](https://github.com/ait-app/ait/pull/356)、[PR #365](https://github.com/ait-app/ait/pull/365)。
- **独立 daemon 下载。** 正式发布增加 Linux x86_64 与 macOS Apple Silicon 的 `ait-daemon` 归档。
  Linux 与桌面共用 manylinux_2_28 构建的 daemon；macOS 独立可执行文件另行 Developer ID 签名与 Apple 公证。
  见 [PR #362](https://github.com/ait-app/ait/pull/362)。

## 兼容性与限制

Pi 1.2.0 是验证基线；接受更新版本不代表所有版本均已验证。Pi 自行管理认证、工具、配置、扩展和原生会话。
交互式扩展对话框会取消；不支持原生历史导入、rewind、live steering、自定义系统提示词、thinking-level 选择、
结构化输出和同一原生会话的并发写入。Native Tools 不提供 Ait 主机权限沙箱。

整图超预算缩放可能降低细节；跨设备 Codex 图片预览需要新版客户端。原生 Android/iOS 图片适配器经过 mock 验证，
未做本次真机验收。居中表单在窄小可用高度下可能轻微覆盖页头；折叠屏实机与 Linux 原生窗口拖动尚未验收。
Claude 本次修复恢复目录判断，未宣称解决取消请求本身的全部异常。Codex 上游用量面板如何分类不由客户端标识保证。

桌面与独立 daemon 支持 Linux x86_64（glibc 2.28+）及 macOS 13+ Apple Silicon。
macOS 裸可执行文件无法 staple 公证票据，首次运行由 Gatekeeper 在线核验。
独立 daemon 归档包含 `ait-daemon` 与 `LICENSE`；无人值守模式另需登录 Ait 账户和已配置的 Provider CLI。
Android APK、Google Play、TestFlight 与 AUR 按各自流程独立发布。

## 发布准备与验证

同步 15 个 Cargo workspace 包、根 npm 包和 6 个 npm workspace、两份锁文件及根
`PKGBUILD` 为 `0.1.8`。结构化比对确认第三方依赖记录相对准备基线不变；npm 锁文件格式保留。
本次版本准备不新增产品代码变更。

本地验证通过：

- `npm ci --no-audit --no-fund`、`npm run verify:release -- v0.1.8` 与 workspace 链接检查。
- 发布脚本 55 项、移动发布脚本 41 项测试；包含独立 daemon 打包、校验及工作流顺序回归。
- `npm run build:desktop-main`、桌面与移动端类型检查。
- 移动端 49 个相关文件 703 项单元测试；真实 Chromium 图片与拖动区域 2 个文件 8 项测试。
- 协议图片、工具投影与 Pi manifest 5 个文件 39 项测试。
- provider crate 全部默认测试 968 通过、32 个外部原生集成测试忽略，严格 all-targets Clippy 通过。
- Rustfmt、变更文件 Oxfmt、文档链接与 `git diff --check` 通过。

本机为 Node 26.11.1；移动端单元测试使用 `NODE_OPTIONS=--no-experimental-webstorage`，避免原生
Web Storage 与 jsdom 冲突，保持默认并行度。正式 GitHub 工作流使用 Node 24。

适用的发布准备 PR/main CI、两平台正式构建、macOS Developer ID 签名、公证及成品 daemon
生命周期门禁全部通过后，核对完整附件的 SHA-256、更新元数据 SHA-512、标签与构建来源，
将实测证据写入 Release Notes，并按[发布指南](../../operations/releasing.md#自动完成正式发布)
自动正式发布。随后校验 DMG 与两个独立 daemon 的 COS 镜像，同步官网和 ait-server 登录页下载入口；
源码合入、官网部署与服务端生产部署分别按实际结果报告。

## Test coverage

本次实际测量 **provider crate、默认 features**，macOS arm64 上行覆盖率为
**91.5863%（31230/34099）**。
这是所选 crate 的 LLVM 行覆盖率，不是全 Rust workspace 或整个产品覆盖率。
测量提交为 `5cf0dd2451e652c9b96fbcc4d12b706c504cf8d9`。
共享[JSON 证据](release-0.1.8-coverage.json)包含 194 个文件的逐文件计数、LCOV 零执行行、
595 项 provider、model、domain、persistence 与 Cargo 构建输入指纹，以及全部 32 个忽略测试名称。

测试结果单列：**968 通过、0 失败、32 跳过**，使用 nextest 默认并行度。
无额外文件排除，沿用 cargo-llvm-cov 默认测试源码过滤。JavaScript/TypeScript 行覆盖率未测，
测试通过数量不能替代覆盖率；没有手动运行整个 Rust workspace 测试。

精确命令：

```sh
export CARGO_TARGET_DIR=target/release-0.1.8-coverage
cargo llvm-cov clean --workspace --locked --offline
cargo llvm-cov nextest -p provider --locked --offline --html --status-level all
cargo llvm-cov report -p provider --json --output-path /tmp/ait-release-0.1.8-coverage-raw.json
cargo llvm-cov report -p provider --lcov --output-path /tmp/ait-release-0.1.8-coverage.lcov
```

`clean --workspace` 只清理本次独立目标目录的 instrumentation 文件，不执行全工作区测试。
平台为 macOS 27.0.1 arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、nextest 0.9.148。
HTML 位于 `target/release-0.1.8-coverage/llvm-cov/html/index.html`；可共享结果以仓库 JSON 为准。

32 个忽略测试需要已安装、配置或认证的外部 Provider，包括 installed-Pi loopback 集成；本次没有解除忽略。
Pi 的离线适配器、JSONL 传输与服务行为测试已运行，不能替代真实模型服务验收。
主要未覆盖区域还包括 Provider dispatch/catalog/request 路径、部分创建 Workspace 与 Agent RPC 分支；
逐文件零执行行见 JSON。Linux、Windows、外部模型服务、真机与 doctest 不在本次测量内。
0.1.6 的测量同时选择 provider/api/domain/filesystem，跨 crate 的覆盖贡献不同，不能当作可比 provider-only 基线；
本次不计算百分点变化。
