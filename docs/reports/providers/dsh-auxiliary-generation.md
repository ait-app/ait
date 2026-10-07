# DSH 辅助生成验证

基于辅助模型接口 PR #208，补齐 DSH 原生 headless 通道。保留原生认证和模型配置，禁用工具、持久化和用户启动注入。Antigravity 未修改。

## Test coverage

测量源码为基于 `59072cd8` 的 Git tree `eb9fa0c81389a9fcfa6439ab9ba4425b84d04849`，Linux x86_64、默认 features、无源码排除。命令：

```sh
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=4 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

本表由此次 HTML index 提取，作为可审查摘要；完整 HTML 仅在本地生成，未上传。

| 范围 | 覆盖行 / 总行 | 行覆盖率 |
|---|---:|---:|
| Workspace | 54860 / 58161 | 94.32% |
| Provider crate | 26372 / 28154 | 93.67% |
| DSH adapter | 158 / 198 | 79.80% |
| DSH metadata | 57 / 103 | 55.34% |
| Metadata process | 48 / 48 | 100% |

无同修订、同环境的可比基线。11 个安装 CLI 测试在覆盖率运行中 ignored，因此 metadata 的实际进程生成路径未计入该覆盖率；该路径另行通过真实安装 CLI 和本地模型服务验证。未验证 macOS/Windows；后续 CI 应提供固定 DSH 安装，纳入真实生成与取消测试。

## 测试执行

Workspace 1928 项通过、11 项 ignored；build、clippy（`-D warnings`）、fmt 全部通过。另运行 `AIT_TEST_DSH_BIN=/tmp/ait-dsh-import-test/node_modules/.bin/dsh cargo test -p provider installed_dsh_metadata -- --ignored --nocapture`，1 项通过；使用临时 DSH_HOME 和本地 SSE 模型夹具，确认请求没有工具且没有会话目录。未使用真实外部模型服务。

## 限制

仅支持已知原生 headless 核心插件组合；自定义模型插件不自动启用。显式推理等级目前只支持 deepseek-official，其他模型的自动选择沿用原生默认值。Unix 取消结束进程组，其他平台仅保证直接子进程结束；进程崩溃后的临时文件清理不在本次保证范围内。
