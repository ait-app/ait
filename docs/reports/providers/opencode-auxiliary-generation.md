# OpenCode 辅助生成验证

基于辅助模型接口 PR #208，补齐 OpenCode V1/V2 的独立通道。复用原生认证与模型配置，隔离前台会话，单步生成并禁用工具。Antigravity 未修改。

## Test coverage

测量源码：基于 `59072cd8` 的 Git tree `2ac1e07a32e931938325de1f1c273e0b02437ac6`。Linux x86_64，默认 features，无源码排除；11 个需安装 CLI 的测试 ignored，未验证 macOS/Windows。命令：

```sh
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=1 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

下表是此次 HTML index 的可审查摘要。完整 HTML 已生成并保留本地副本，未上传。

| 范围 | 覆盖行 / 总行 | 行覆盖率 |
|---|---:|---:|
| Workspace | 54825 / 58244 | 94.13% |
| Provider crate | 26336 / 28237 | 93.27% |
| OpenCode client | 383 / 418 | 91.63% |
| OpenCode metadata | 49 / 188 | 26.06% |
| OpenCode runtime | 174 / 256 | 67.97% |

无同修订、同环境的可比基线。实际 CLI 通道验证单独运行，不计入上述比例；因此 metadata 的启动、请求和清理路径覆盖率较低。后续 CI 应固定安装 V1/V2 CLI 并加入取消时清理与异常退出测试。

## 测试执行

独立构建目录执行 workspace 测试：1927 项通过、11 项 ignored；build、clippy（`-D warnings`）、fmt 通过。安装 CLI 测试使用临时目录与 loopback 模型，确认没有 tools 且正常完成后原生会话为空；没有调用外部模型服务。

早期共享构建目录造成一次覆盖率二进制丢失，另一次覆盖率运行遇到无关 Antigravity 发现夹具超时；最终单线程覆盖率运行通过。常规检查已迁移至 `/home/lonnet/.cache/ait-pr-targets/opencode` 独立目录，防止其他 PR 的编译产物混入。

## 限制

OpenCode V1 在创建响应到达前取消时可能没有会话 ID 可删除；进程崩溃也可能遗留原生历史。V2 预先记录新会话 ID，取消后尝试中止和删除，清理最多五秒。取消或删除失败时不声称历史必定清理。
