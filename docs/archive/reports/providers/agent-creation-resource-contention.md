# Agent 创建时的任务预算竞争

日期：2026-10-03。初始复现基线：`cbc85078`；PR 基线为 main `141035d7`。
提交前验证源码：`ff9e86693da0bd2844fb9dc94590ca08f9fad146`；后续修改只补充验证文档。
平台：macOS arm64；Cargo 默认 features，离线 ACP fixture，不使用真实模型 API。

## 现场与判断

用户在 `wizardly-giraffe` 创建 DeepSeek Harness Agent 时遇到
`Resource budget exhausted requestType=agent.create.request code=resource_exhausted`。
桌面日志在本地时间 20:32:59 和 20:35:33 记录相同错误，中间另有连接断开记录。
只读检查时该工作树有 97 条 Git status 记录；已跟踪文件的未提交 Diff 为 187,937 字节，
未跟踪文件共 34 个、381,969 字节。与本地 `origin/main` 的已提交分支 Diff 为空。
这些数据没有显示工作树内容接近既有 4 MiB Diff 输出上限。

客户端的创建生命周期请求默认携带 `subscribe: true`。
Provider 连接在启动原生进程前，通过 `Runtime::run` 安装创建进度观察器。
该调用使用 server 范围内只有一个 permit 的 `jobs`，与前台 Checkout Diff 请求共享。
其他工作区的 Diff 或元数据任务持有 permit 时，创建观察器立即返回 `resource_exhausted`。
这条路径适用于所有 Provider，并非 Harness 返回的模型或上下文预算错误。

通过实际 DSH adapter 加离线 ACP 子进程复现：占用共享 permit 后，原实现的创建请求
立即完成并报错，子进程尚未启动。新增回归用例在修复前失败，修复后通过。
现场日志没有记录耗尽的是哪种资源，因此该复现证明了一个可产生相同错误的缺陷，
尚不能排除现场同时触发连接的订阅数量或 capability 请求队列上限。

后台 Git fetch 使用独立并发预算；失败路径记录 WARN 后等待下一轮重试，
不会请求 daemon 关闭。现场 daemon 日志没有 panic 或 fatal 退出记录；检查时桌面 daemon
仍在监听。已有断连记录不足以证明进程退出，更不能据此认定 fetch 是退出原因。
后台适配器丢弃 Git stderr，并把非零退出统一映射为 `Failed`，无法还原历史失败原因。
后续使用同一仓库的 SSH origin 进行非交互只读 `git ls-remote origin HEAD` 检查，
退出码为 0、stderr 为空，只能证明检查时网络与认证正常。

## 修复

创建进度观察器改用既有 `Runtime::run_queued`，等待临时占用的任务 permit 释放。
API 的有界请求队列、订阅数量限制、进程关闭取消以及持久化幂等键继续生效。
没有变更能力归属、依赖边界、协议或资源预算值。

新增五项 DSH 创建回归：共享任务占用时等待且不提前启动子进程、关闭取消等待、
真实订阅容量耗尽时拒绝、无订阅创建无需共享 permit、原生初始化失败返回 `agent_io`
且 runtime 保持 Ready。另新增后台 fetch 连续三次失败后前台请求仍成功、下一轮成功恢复的测试。

修改只位于当前源码工作树，未替换桌面应用内的 daemon，未重启用户正在运行的服务。
创建仍可能等待慢任务结束；本修复没有优化大型 Diff 的计算耗时，
也没有取消真正耗尽的请求队列或订阅容量限制。

## 本地定向验证

所有 Cargo 命令使用 `CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target`。

- `cargo test --locked --offline -p provider service::agent_execution::tests::subscriptions`：14 passed。
- `cargo test --locked --offline -p provider local::deepseek_harness::tests`：6 passed。
- `cargo test --locked --offline -p filesystem service::git_fetch::tests`：7 passed。
- `cargo clippy --locked --offline -p provider -p filesystem --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`、`git diff --check`、`npm run check:docs`：通过。

## Test coverage

提交准备阶段使用 cargo-llvm-cov 0.8.4、Rust 1.98.1，在 macOS arm64 测量完整
Cargo workspace，默认 features 和默认文件过滤，没有额外排除；不插桩 doctest。

| 范围                | 行覆盖率   | covered / total     |
| ------------------- | ---------- | ------------------- |
| Workspace           | **94.54%** | **48,593 / 51,397** |
| provider            | **93.93%** | **21,221 / 22,592** |
| filesystem          | **95.01%** | **11,135 / 11,720** |
| Provider connection | **96.92%** | **126 / 130**       |
| Git fetch service   | **97.73%** | **258 / 264**       |

完整源码指纹、修改的 Rust 文件 SHA-256、命令、范围和测试结果见
[可审阅覆盖率证据](agent-creation-resource-contention-coverage.json)。
未在同一环境重新测量目标 main，不计算与历史报告的覆盖率差值。
HTML 已生成于 `/private/tmp/ait-agent-creation-pr-target/llvm-cov/html/index.html`；
PR 中的 JSON 是共享测量证据，HTML 和原始执行日志留在本地。

提交前检查结果与行覆盖率分开记录：

- `cargo test --locked --offline --workspace`：1,757 passed、0 failed、3 ignored。
- `cargo build --locked --offline --workspace`：通过，无编译警告。
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`：通过。
- `cargo llvm-cov --locked --offline --workspace --html`：1,757 passed、0 failed、3 ignored。
- `cargo fmt --all --check`、`git diff --check`、文档 Oxfmt 和 `npm run check:docs`：通过。

首次受限沙箱运行因禁止绑定本地端口而中止；允许离线测试夹具启动后，完整测试与覆盖率
均通过。3 项 ignored 沿用现有声明：已安装 Claude 的模型发现、真实 Claude 回合和
真实 Codex 回合；没有新增跳过。以下环境同时用于 test、build、clippy 和 coverage：

```sh
export CARGO_TARGET_DIR=/private/tmp/ait-agent-creation-pr-target
export SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib
cargo llvm-cov --locked --offline --workspace --html
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-agent-creation-pr/coverage-summary.json
```

新修复路径及取消、初始化失败、订阅限额已有回归测试。Provider connection 尚有
非法 key、缺失 executor 和部分发送失败分支未覆盖，后续可补充直接连接级错误测试。
本次没有真实认证 Harness 回合、Electron 重打包或 Linux/Windows 平台验证。
