# Rust workspace 每 crate 95% 覆盖率验证

2026-10-14；基于源码提交 `96a432400be59872ae180c3ec60cde34b0454490` 的未提交工作树。
用户明确要求整个 Rust 后端 workspace 的每个 crate 达到 95%，并授权全量测试和覆盖率测量。
基线在增加测试前、同一提交与平台取得；[共享 JSON 证据](crate-coverage-95-2026-10-14.json)记录两次测量的逐文件计数、
最终 LCOV 零执行行、全部 Rust 源码 SHA-256、fixture 与 manifest 指纹、原始报告哈希和忽略用例原因。

新增 77 项测试，覆盖 Bot 真实适配器生命周期与持久状态、Relay 控制/桥接/下载超时、
Daemon 登录与退出、API 协议拒绝、Provider 原生协议与离线进程夹具、Metadata 自动化和
Filesystem Git 摘要/协议投影。外部 HTTP/WebSocket 服务使用本机 fixture，原生 CLI 使用不可变离线夹具。

回归测试发现 headless 启动先装配后台服务、后校验登录状态，失败后后台服务保留实例锁。
现在先校验登录与安装身份，再装配服务；测试确认缺失登录和身份不符均能释放锁。
其余生产改动仅在私有适配器内提供 API 地址/连接器/进程路径注入，未改变公开接口或模块边界。

## Test coverage

全部 **15 个 crate 均达到 95% 行覆盖率**，按原始整数计数验收，不以四舍五入判断。
Workspace 从 **94.1039%（62549/66468）** 提升到
**95.2509%（63339/66497）**，增加 **1.1470 个百分点**。
daemon 可执行 crate 从 92.4740%（2224/2405）提升到表中结果。

| Crate       | 基线行覆盖率            | 当前行覆盖率            | 变化        |
| ----------- | ----------------------- | ----------------------- | ----------- |
| api         | 93.7896%（2220/2367）   | 95.1415%（2252/2367）   | +1.3519 pp  |
| bot         | 81.3651%（1812/2227）   | 95.2000%（2142/2250）   | +13.8349 pp |
| browser     | 97.5443%（715/733）     | 97.5443%（715/733）     | +0.0000 pp  |
| daemon      | 92.4740%（2224/2405）   | 95.0561%（2288/2407）   | +2.5821 pp  |
| domain      | 100.0000%（556/556）    | 100.0000%（556/556）    | +0.0000 pp  |
| filesystem  | 94.8161%（9566/10089）  | 95.1531%（9600/10089）  | +0.3370 pp  |
| host-link   | 94.4444%（187/198）     | 96.9697%（192/198）     | +2.5253 pp  |
| metadata    | 94.3570%（6956/7372）   | 95.0081%（7004/7372）   | +0.6511 pp  |
| model       | 96.4505%（1413/1465）   | 96.4505%（1413/1465）   | +0.0000 pp  |
| persistence | 95.7966%（1846/1927）   | 95.7966%（1846/1927）   | +0.0000 pp  |
| provider    | 94.4555%（30000/31761） | 95.0386%（30189/31765） | +0.5831 pp  |
| relay       | 82.8080%（578/698）     | 95.4155%（666/698）     | +12.6074 pp |
| schedule    | 95.8694%（998/1041）    | 95.8694%（998/1041）    | +0.0000 pp  |
| terminal    | 96.6387%（1610/1666）   | 96.6387%（1610/1666）   | +0.0000 pp  |
| voice       | 95.1605%（1868/1963）   | 95.1605%（1868/1963）   | +0.0000 pp  |

范围为全部 15 个 workspace package、默认 features；无新增文件排除、忽略测试或覆盖率豁免。
沿用 cargo-llvm-cov 的默认测试源码过滤；测试辅助生产模块仍按工具默认计入。
macOS 27.0.1 arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、nextest 0.9.148。
Linux/Windows 条件分支未在本机编译；Python/JavaScript/shell fixture、上游二进制和 doctest
不属于本次 Rust instrumentation 范围。HTML 报告位于 `target/llvm-cov/html/index.html`，
共享 JSON 提供不依赖本机路径的逐文件计数与 LCOV 零执行行定位。
验收使用 LLVM JSON summary；LCOV 的 `DA=0` 列表与 summary 行计数不完全一致，
仅用于定位，不能用该列表长度重新计算覆盖率。

精确命令如下；先清理 workspace 的旧 instrumented 产物，避免旧二进制混入行数。
`SHERPA_ONNX_LIB_DIR` 指向本机已有静态库，其他平台需换成对应平台库路径。

```sh
cargo llvm-cov clean --workspace --locked --offline
SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib cargo llvm-cov nextest --workspace --locked --offline --html
cargo llvm-cov report --json --output-path /private/tmp/tall-sheep-workspace-final.json
cargo llvm-cov report --lcov --output-path /private/tmp/tall-sheep-workspace-final.lcov
python3 scripts/check-crate-coverage.py /private/tmp/tall-sheep-workspace-final.json
```

测试结果与覆盖率分别记录：基线 2265 通过、0 失败、32 忽略；
最终 **2342 通过、0 失败、32 忽略**。既有忽略项包括安装版 OpenCode、DSH、
Claude、Codex、AGY 的原生或认证用例，以及 fish 4 兼容性；名称与原因完整列于共享 JSON。
默认 nextest 并行度未降低。Doctest 检查通过，14 个 library target 当前均无 doctest。

未覆盖行仍集中在 Git checkout/worktree 的失败分支、目录 RPC 与自动化进程异常、原生 Provider
会话错误及离线语音引擎。95% 是 crate 总行覆盖率，不代表每个文件或每个分支均达到 95%。
后续修改这些路径时应补充对应故障注入，并在 Linux/Windows 复测；真实认证和模型执行需在
具备相应依赖的隔离环境中显式运行既有忽略用例。

## 其他验证

以下命令通过；最后增加的 filesystem 测试另作定向运行：29 通过。

```sh
cargo fmt --all --check
SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=target/checks cargo clippy --locked --offline --workspace --all-targets -- -D warnings
SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=target/checks cargo build --locked --offline --workspace
SHERPA_ONNX_LIB_DIR=/Users/necokeine/Documents/ait/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=target/checks cargo test --locked --offline --workspace --doc
python3 scripts/check-crate-coverage.test.py
node --test scripts/check-docs.test.mjs
node scripts/check-docs.mjs
git diff --check
```

覆盖率门槛脚本回归 2 项通过，文档链接检查器回归 1 项通过；新增 JavaScript fixture 的
语法、格式和 lint，以及 Python fixture 语法检查通过。未写入真实凭据、模型数据或运行时数据库。
