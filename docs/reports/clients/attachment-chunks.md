# 附件分块传输验证

超过 1 MiB 的客户端规范 JSON 消息按 256 KiB 分块，daemon 完整组装后分发。文件流逐帧确认并等待工作槽，避免单连接队列溢出或与后台任务竞争导致上传丢失。见 [ADR-096](../../decisions/clients/adr-096-acknowledged-client-chunks.md)。

## Test coverage

测量源码为基于 `f83d9579` 的 Git tree `6e8c9994344cc8ecad364593d788e349399bb758`（加入报告与文档索引调整前）。Linux x86_64、默认 features、无源码排除、10 个安装 CLI 测试 ignored；未验证 macOS/Windows。

```sh
CARGO_TARGET_DIR=/home/lonnet/.cache/ait-pr-targets/chunks \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config RUST_TEST_THREADS=1 \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

以下为本次 HTML index 的可审查摘要。完整 HTML 生成于独立 target 的 `llvm-cov/html/index.html`，尚未上传。

| 范围 | 覆盖行 / 总行 | 行覆盖率 |
|---|---:|---:|
| Workspace | 54869 / 58092 | 94.45% |
| API crate | 2109 / 2267 | 93.03% |
| Filesystem crate | 11560 / 12187 | 94.86% |
| Provider crate | 26243 / 27949 | 93.90% |
| API chunk assembly | 98 / 106 | 92.45% |
| File connection | 242 / 256 | 94.53% |
| Upload state | 63 / 63 | 100% |
| Provider prompt | 101 / 102 | 99.02% |

没有相同基线提交和测量环境的可比结果。剩余缺口包括全局预算耗尽和部分 socket 关闭时机；尚未实测公网 relay 吞吐、真实模型服务的最大图片数或跨平台文件选择器。TypeScript 行覆盖率未测量；已有定向行为测试，后续可在 CI 中补充客户端覆盖率。

## 测试执行

最终 workspace 1928 项通过、10 项 ignored；`cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。全量检查发现并修正文件工作槽竞争：已开始的上传使用 `run_queued` 等待，而不因短暂占用丢弃上传状态。

客户端 80 项定向测试与 SDK 3 项上传测试通过，覆盖 Unicode 原字节重组、旧 Host 门控、ACK 校验和超时、断开清理、规范 envelope 转换、逐块等待及无效 chunkSize 在发送前拒绝。SDK build、mobile TypeScript、oxfmt、oxlint 与文档链接检查通过。

API 四项分块/上传回归通过；真实 daemon 单连接上传 200 MiB 夹具并校验文件大小、首尾块。该夹具含 PDF 标记，但验证的是二进制传输，不是 PDF 渲染。Provider 验证 50 张原图的 prompt 超过旧 1 MiB 上限仍保持解码后的原始字节，51 张被拒绝。

## 运行边界

单条 JSON 消息最多 64 MiB，Provider prompt 最多 63 MiB，最多 50 张图片且单图 base64 最多 32 MiB。超限图片批次必须由用户拆分发送；不会强制压缩图片。单文件上传最多 256 MiB，200 MiB PDF 走文件流，不进入大 JSON。分块预留预算最多 256 MiB 原始字节，另有 JSON 与 adapter 的内存开销。旧 Host 不支持大消息时给出升级提示；双方升级后才能使用新传输。断开时不自动重放请求。
