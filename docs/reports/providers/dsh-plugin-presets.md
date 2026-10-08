# DSH 插件情景模式接入验证

日期：2026-10-08。提交准备基于 main `4794dbc4` 的独立分支
`fix/dsh-native-plugin-presets`；平台 Linux x86_64，Rust 1.98.1 / LLVM 23.1.1。
本 PR 的 Rust 源码就是测量范围；[统计产物](dsh-plugin-presets-coverage.json)
记录完整 base commit 和 staged Rust patch 的 SHA-256，可用
`git diff --binary <base> <PR commit> -- crates bins Cargo.toml Cargo.lock` 校验。
设计与官方源码依据见 [ADR-109](../../decisions/providers/adr-109-dsh-native-plugin-presets.md)。

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
