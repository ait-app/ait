# Codex 异步上下文压缩验证

日期：2026-10-15。基线：`0d9338b33e425a71f4adbb8015319a94fa3770ac`，验证范围为本报告所在提交的实现。
环境：Linux x86_64，使用仓库指定的 Rust 1.98.1。本次只修改 Codex Provider 内部的能力发现、配置映射、压缩事件处理和相关测试，
沿用原有会话特性协议及客户端通用开关，不改变 crate 或上下文所有权边界。

## 行为

- 按原生特性目录探测支持和当前原生启用状态；处理分页、旧版接口、移除的特性和异常游标。
  只有探测到可用的 `async_compaction` 才显示会话开关，已有的保存值不能绕过检测。
- 会话开关优先于高级配置，未设置时保留原生继承；所有配置值必须是布尔值。
- 创建和恢复会话传递原生配置。修改选项后，在下一轮开始前重启原生连接并恢复相同 thread，
  避免 app-server 缓存已加载 thread 的旧配置；相同配置不会额外重启。
- 自动压缩沿用原生时间线投影，不占用手动压缩的等待状态，也不会结束当前轮次。
  轮次结束后到达的压缩完成通知仍会保留，同一通知重复到达时去重。

## 验证

- `cargo test --locked -p provider --lib --quiet -- local::codex local::configuration`：178 项通过（含 9 项新增测试），1 项按既有设置
  忽略的真实 Codex 模型请求测试未运行，728 项无关测试被过滤；使用默认 features。
- `cargo fmt --all --check`：通过。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`：通过，无 lint 豁免。
- `cargo build --locked --workspace`：通过，无编译警告。
- `npx --yes oxfmt@0.46.0 --check docs/operations/codex.md docs/reports/providers/codex-async-compaction.md docs/README.md docs/reports/README.md`：通过。
- `npm run check:docs`：链接检查通过。
- `python3 -c 'import ast, pathlib; ast.parse(pathlib.Path("crates/provider/tests/fixtures/codex_app_server.py").read_text())'`：离线协议夹具语法检查通过。
- `git diff --check`：通过。

本机 `codex --version` 为 `codex-cli 0.162.1`，`codex features list` 未包含 `async_compaction`。
生成该版本的 app-server JSON schema，确认特性查询的分页结构、生命周期和布尔启用值。
命令：`codex app-server generate-json-schema --experimental --out /var/tmp/ait-async-pr-codex-schema`。
本次离线协议测试验证 Ait 的集成行为，没有调用模型或测量真实长上下文压缩延迟。
真实后台压缩仍需在提供该特性的新版 Codex 上，用长会话验收。

## Test coverage

**Not measured（未测量）**：本次未要求覆盖率测量，按仓库规范仅执行受影响模块及直接相关行为的测试。
没有本次行覆盖率、覆盖/总行数、可比较基线或覆盖率产物；通过测试数不代表覆盖率。
如后续要求测量，应运行 `cargo llvm-cov --locked -p provider --lib --html -- local::codex local::configuration`
并发布可审查的报告产物，保持上述测试范围；跨平台行为和真实长上下文压缩仍需独立验收。
离线测试源码为可审查的行为证据：[Codex 测试](../../../crates/provider/src/local/codex/tests/compaction.rs)、
[配置测试](../../../crates/provider/src/local/configuration/tests.rs)。
