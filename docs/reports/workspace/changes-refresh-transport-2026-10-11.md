# 本机 Changes 刷新断线：修复验证

日期：2026-10-11

基线：`aba28accc47d793d4f9b2c162f79493ab2f3eaab`（main）

验证源码：本报告所在提交的 11 个变更 Rust 文件。按 Git 相对路径排序，对每个文件依次
拼接路径、NUL、完整内容、NUL 的 SHA-256：
`1e4524e4a896c88dea9090c484585102fb45de6d23859f3582a4114618331f2e`。

## 问题与修复

用户在本机 Host 刷新侧边栏 Changes 时看到 `Transport closed (code 1006)`。
本机诊断记录存在大量 `ResourceExhausted`，但不能仅凭这一提示断定唯一断线原因。
本次复现并修复了两个相关路径：

- 全局短任务额度被占用时，Checkout 刷新返回 `resource_exhausted`。前台 Checkout
  现在使用独立的串行额度，等待与执行不占用其他能力的全局额度。
- 两个分别合法的 JSON diff 累计超过连接输出预算时，同步发送会取消连接。Checkout
  成功响应与 diff 更新现在等待现有预算；等待的大帧不预占剩余字节，小型 Ping 仍能发送。

输出字节与消息上限、在途写入预算、物理写入超时、连接准入及 wire 协议保持原有契约。
等待可被断线、订阅释放或服务关闭取消，已开始的阻塞 Git 工作仍被跟踪。
详见 [ADR-132](../../decisions/workspace/adr-132-checkout-refresh-admission-and-backpressure.md)。

## 测试执行

平台：macOS arm64，Rust 1.98.1，默认 Cargo features，使用锁定依赖与离线构建。
本机监听测试使用独立临时 Git 仓库及测试服务。

```sh
cargo nextest run --locked --offline -p model -p api -p filesystem \
  -E 'test(outbound::tests) | test(runtime::tests) | test(checkout_refresh) | test(git::connection::checkout) | test(git::rpc::checkout) | test(chunks::startup_burst)'
cargo nextest run --locked --offline -p daemon --test process -E 'test(checkout::)'
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo build --locked --offline --workspace
cargo fmt --all --check
npm run check:docs
git diff HEAD --check
```

- 相关 model、api、filesystem 测试：60 项通过；过滤掉 476 项无关测试。
- daemon Checkout 进程测试：5 项通过；过滤掉 91 项无关测试。
- 工作区构建、严格 Clippy、Rust 格式、文档链接与 diff 空白检查通过。
- 新回归覆盖旧连接及单物理连接：全局额度繁忙时刷新、初始 diff、后续 diff 更新、Ping
  与状态读取仍成功；另覆盖前台串行、关闭取消、阻塞任务跟踪，以及 JSON 字节/消息
  背压、过大帧拒绝、关闭接收端和等待大 diff 时的控制响应。

未运行完整 Rust 工作区测试；仓库规则要求仅运行变更及直接相关测试。
未手动操作已安装的 Electron 界面，也未验证 Linux、Windows、移动端或真实 SSH/在线 Host。
测试证明上述两个路径已修复，不代表对所有 1006 成因作出结论。当前安装的本机 Host
尚未替换或重启，更新到包含此修复的 daemon 后才会生效。

## Test coverage

**not measured**：用户未请求覆盖率测量；本次未运行 `cargo llvm-cov`，没有当前覆盖率
百分比、行数、可比基线或覆盖率产物。上面的通过数量仅为测试执行结果。

如后续请求测量，使用 `cargo llvm-cov nextest`，以同样的筛选条件测量
`model`、`api`、`filesystem` 和 daemon Checkout 进程测试，记录该次提交、features、
排除范围与行数，并提供可共享的报告。工作区覆盖率另需用户明确请求。
