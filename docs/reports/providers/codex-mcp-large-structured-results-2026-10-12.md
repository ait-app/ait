# Codex MCP 结构化大结果导致会话失败

日期：2026-10-12。验证源码提交：`424857302c2f59511fbfc790c00f291ca6ea1c64`，
基于 main `4ab7e7be1aa00e94b59b60817fe004d041377878`；随后仅更新本验证报告。
平台：macOS aarch64，默认 features。

## 原因与修复

14:08:55（UTC+8），Codex `0.160.0` 的 `codex_apps.github.fetch` 成功返回版本比较数据，
其 MCP `structuredContent` 约 808 KiB。Ait 投影后的单条工具记录为 **828,249 字节**，
超过 Timeline **786,432 字节** 上限。daemon 连续记录 `Timeline item exceeds size limit`
与 `Native provider turn failed`；AgentManager 将展示存储的 `ResourceExhausted` 转为
失败事件并关闭原生会话。原生记录中工具状态为 completed，AIT 记录的错误为
`Provider execution failed`。

Codex adapter 现在保留预算内的原始 JSON 字段，对超大参数、结果和错误显示带明确标记的
文本前缀。完整值继续留在原生 transcript；实时与历史读取共用同一转换。
每个参数／错误字段上限 64 KiB，结果上限 512 KiB；预览考虑 UTF-8 边界和 JSON
再次转义，检查超限即停止序列化。详细决策见 [ADR-140](../../decisions/providers/adr-140-codex-mcp-display-previews.md)。

修复仅在本工作区源码和本地构建中生效，未替换安装版应用或重启正在运行的 daemon。

## 验证

- 修复前新增大结构化结果回归失败；修复后预览可写入并原样重放，保持原有 generation 和序号。
- **48 项定向测试通过**，766 项不相关测试跳过。范围为 Codex 工具投影、历史转换、
  streaming，以及两个大工具结果的 AgentManager 回归。
- 完整离线 stdio 回归返回 1 MiB 结构化 MCP 结果，验证 running／completed 写入、
  原生历史读取、同一线程与同一进程继续下一轮，注册状态保持 idle 且无 lastError。
  fixture 原生记录仍包含完整 1 MiB 数据。
- 覆盖精确字节边界、超限一个字节、中文、控制字符与 JSON 转义、超大输入和错误，
  同时保留正常 350 KiB 输出的结构、MCP 图片和元数据处理。
- `cargo clippy --locked -p provider --all-targets -- -D warnings`：通过。
- `cargo build --locked -p daemon --bin daemon`：通过。
- `cargo fmt --all --check`、`git diff --check`、Python fixture AST 解析：通过。

定向测试命令：

```sh
cargo nextest run --locked -p provider --lib -E 'test(local::tool_detail::) | test(local::codex::discovery::) | test(local::codex::streaming::) | test(oversized_codex_mcp_result_preserves_the_session_history_and_next_turn) | test(large_completed_tool_result_is_saved_without_closing_the_native_session)'
```

## Test coverage

**Not measured**：用户未要求覆盖率测量，本次仅执行直接相关测试；48 项通过不代表覆盖率。
没有覆盖率百分比、行计数或本次测量产物，也没有可比较基线。如后续要求测量，应使用
`cargo llvm-cov nextest -p provider --lib` 并沿用上述过滤器，在相同源码版本记录行统计和产物。
未验证真实模型调用和 Linux／Windows 平台；离线测试覆盖会话执行与恢复的相关边界。
