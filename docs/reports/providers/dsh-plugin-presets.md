# DSH 插件情景模式接入验证

## 2026-10-14：合入 main 并解决 PR #229 冲突

测量前父提交：PR `747af15d5d272b62789da2541aee2e01137eda28`，
合入 main `8a4f8b9c99ececd99153fb4d99893a1e61aa2e18`；
本节对应本次合并提交的工作树，Linux x86_64、默认 features。

保留 main 的 DSH 账号余额、错误诊断、crate 内部可见性和不可变测试启动器，
同时保留情景模式/权限独立目录与会话配置校验。新增插件模式下余额与失败诊断共存的回归测试。
测试参数通过 `AIT_DSH_FIXTURE_ARGS` 写入每项测试的临时目录；不改写共享启动脚本。
原 ADR-109 与 main 冲突，DSH 决策文档改为 ADR-147 并更新所有本地链接。

### Tests

以下命令均设置 `CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target` 复用构建缓存；
没有降低测试并行度。nextest 在本机不可用，使用 Cargo 的定向测试。

| 命令 | 结果 |
| --- | --- |
| `cargo test --locked -p provider local::deepseek_harness --lib` | 78 通过，6 个安装相关测试默认忽略 |
| `cargo test --locked -p provider plugin_mode_keeps_native_balance_and_failure_diagnostics --lib` | 新增回归测试 1 通过 |
| `cargo test --locked -p provider service::provider_catalog --lib` | 25 通过 |
| `cargo test --locked -p provider service::agent_execution::tests::controls --lib` | 8 通过 |
| `cargo test --locked -p provider composition::tests --lib` | 4 通过 |
| `cargo clippy --locked -p provider --all-targets -- -D warnings` | 通过 |
| `cargo build --locked --workspace` | 通过 |
| `cargo fmt --all --check`、`npm run check:docs`、`git diff --check` | 通过 |
| `sh -n crates/provider/tests/fixtures/deepseek_harness_native.sh` | 通过 |

### Test coverage

**Not measured**：本次是冲突整合与定向回归验证，依照最新 AGENTS.md 未执行全量测试或
workspace 覆盖率；116 项相关测试通过不代表覆盖率。没有本轮覆盖/总行数或新增覆盖率产物。
下面的 94.18% 与 JSON 产物仅属于 `747af15d` 的历史测量，不能当作合入新 main 后的结果。
未重新验证真实模型、安装版 DSH、macOS/Windows 和 GUI；后续有需要时针对这些范围验证。

## 2026-10-08：初始 PR 历史验证

日期：2026-10-08。提交准备基于 main `4794dbc4` 的独立分支
`fix/dsh-native-plugin-presets`；平台 Linux x86_64，Rust 1.98.1 / LLVM 23.1.1。
本 PR 的 Rust 源码就是测量范围；[统计产物](dsh-plugin-presets-coverage.json)
记录完整 base commit 和 staged Rust patch 的 SHA-256，可用
`git diff --binary <base> <PR commit> -- crates bins Cargo.toml Cargo.lock` 校验。
设计与官方源码依据见 [ADR-147](../../decisions/providers/adr-147-dsh-native-plugin-presets.md)。

## 行为

模式目录读取实际 `agentPresets/list`，包含自定义 ID、显示名称、描述和默认值。
权限独立读取原生目录。创建传递原生 preset；导入与恢复保留插件组合；
旧权限 modeId 继续兼容。配置校验不创建探测会话，已创建会话禁止更换组合。
自定义 Web Host profile 通过 daemon 环境变量选择，不合并桌面 profile。

## 测试执行

提交前使用默认 Cargo features。完整 workspace 测试通过 **2005 项**，默认忽略 15 项。
覆盖率运行通过 **2004 项**、忽略 15 项；`cargo llvm-cov` 默认不运行 doctest，
因此与普通全量测试相差一个通过的 doctest。

```sh
PATH=/tmp/ait-dsh-test-bin:$PATH CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
  cargo test --workspace -- --test-threads=4
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target cargo build --workspace
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
  cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm run check:docs
git diff --check
```

上述构建、lint、格式、文档检查全部通过。测试夹具的参数文件已改为写入临时可执行文件
旁边的 `dsh.args`，不再污染仓库。

最初全量测试受本机全局 `core.hooksPath=.githooks` 影响，两项 Git hook 测试失败。
生产 Git 适配器会清除 `GIT_*` 环境变量，单纯设置 `GIT_CONFIG_GLOBAL` 不足以隔离。
最终使用仅供测试的 `/tmp/ait-dsh-test-bin/git`，隔离本机配置并保留夹具临时 HOME 下的配置：

```sh
#!/bin/sh
if [ "$HOME" = /home/lonnet ]; then
  export GIT_CONFIG_GLOBAL=/dev/null
fi
exec /usr/bin/git "$@"
```

未修改用户全局 Git 配置，也未为此修改生产 Git 代码。

此前还用本机 DSH `0.2.0-rc.2`、临时 `DSH_HOME` 运行原生集成测试：

```sh
AIT_TEST_DSH_BIN=/tmp/ait-installed-dsh cargo test -p provider \
  installed_host_discovers_switches_permissions_and_adopts_legacy_sessions --lib -- --ignored
```

1 项通过；不调用模型，不读写用户已有会话。临时入口原样传递参数给桌面安装包的
`resources/runtime/primary-runtime/dependencies/node/bin/node` 和
`resources/app/dsh/node_modules/@deepseek-ai/dsh/lib/bin.js`。
本次 workspace 执行保持默认忽略规则，不运行需要真实模型或指定原生会话的安装测试。

## Test coverage

- Workspace：**94.18%**，**56,089 / 59,555** 行。
- Provider：**93.25%**，**27,065 / 29,025** 行。
- 新增原生预设映射 `native/presets.rs`：**99.02%**，**101 / 102** 行。
- 没有相同范围的可比较基线，不报告增减百分比。

测量与导出命令：

```sh
PATH=/tmp/ait-dsh-test-bin:$PATH LLVM_COV=/usr/bin/llvm-cov \
  LLVM_PROFDATA=/usr/bin/llvm-profdata CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
  cargo llvm-cov --workspace --html -- --test-threads=4
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
  CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
  cargo llvm-cov report --json --summary-only --output-path /tmp/ait-dsh-coverage-raw.json
```

完整 HTML 在共享 target 的 `llvm-cov/html/index.html`。
可随 PR 审查的[覆盖率统计 JSON](dsh-plugin-presets-coverage.json)由原始 JSON 提取，
包含 workspace、逐 crate、改动文件的覆盖与总行数；它不是手写的估计值。
未显式排除源文件；默认 features、默认忽略测试，不包含 doctest 覆盖率。
Windows/macOS、真实付费模型回合、GUI 端到端操作和部分原生传输异常仍未验证。
后续需要对应平台 CI 与真实 Host 故障场景验证；测试通过数与覆盖率分开统计。

## 接入范围

夹具覆盖自定义与损坏预设、目录更新、模式和权限分离、创建/导入/恢复、
拒绝会话重组、无权限服务的组合、只读配置校验以及 profile 参数传递。
原生插件工具、审批与问题沿用 Host 通道；Ait 不托管插件专属 Web UI 或插件安装面板。
当前 Host 需支持预设和权限目录接口；接口缺失或协议错误会明确失败，不回退硬编码选项。
