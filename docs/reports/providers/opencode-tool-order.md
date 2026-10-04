# OpenCode 工具与结论顺序修复

日期：2026-10-04。基线：上游 main `151ce454867d915b5f80823463d9b8eafd0283ae`（0.0.15）。
PR #155 已于 2026-10-03 合并（`be9347f8`），其合并后 CI 通过；此次为后续修复。
上游从 `4840a3a0` 到该基线没有修改 OpenCode adapter，仍会出现工具与结论错序。

## 问题与修复范围

桌面复测反馈：OpenCode 1.18.33 工具执行后的助手结论显示在工具卡前，重启后仍错序。
原因是此前只在首个文本增量前发布用户输入，工具条目到回合结束才发布；共享 Timeline
在首次写入时分配顺序，因此最终历史不能仅靠去重修正已写入的增量顺序。

修复限于 `crates/provider/src/local/opencode*`：

- 每个原生文本项首次增量前，读取当前输入开始、该文本项之前的原生历史，按顺序发布
  已完成的用户、说明文字、工具与推理条目；同一 assistant message 内的前序 parts 也适用。
- 发布过的完整条目保留在该会话的既有条目集合中，完成时校验且不重复发布；迟到的
  文本增量不能再次追加到已完成条目。
- 若目标文本尚未出现在 HTTP 历史，或前序助手／工具尚未完成，该回合余下文本等待
  最终历史确认。最终历史仍要求所有 assistant 完成；不会重发输入或调用模型重试。
- OpenCode 显示键升级为 `projection-v3`，通过现有 reconcile 重建原始键和 v2 键下的
  旧错序显示历史。原生消息 ID、会话内容及其他 Provider 不变，之后刷新不重复重建。

沿用原生 HTTP/SSE；Codex、共享 Timeline、数据库 schema、前端及 Provider ports 均未改动。
没有新的领域边界；现有 [ADR-074](../../decisions/providers/adr-074-opencode-native-provider.md)
补充发布顺序说明。每个新文本项增加一次历史读取，不是每个 token 读取一次；
前序历史延迟落盘时会降低该回合的流式及时性，以保留持久化顺序。

## 回归与复测

修复前新增回归实际失败：保存结果只有用户、结论，缺少本应在结论前的说明和两张工具卡。
修复后使用真实 HTTP/SSE fixture 和 SQLite 覆盖：

- V1（1.x）和 V2 协议形状下的审批允许／拒绝、两个工具后流式输出结论。
- 同一原生 assistant message 内的工具和未完成结论。
- 先流式说明、再工具、再结论，迟到说明增量不会重复写入。
- SQLite 关闭再打开后顺序保持，重新读取原生历史不重建已正确的游标。
- 原始／v2 键的旧错序历史只重建一次，不重发输入。
- 原生历史暂缺文本、前序未完成及身份异常的处理。

本轮未启动真实 OpenCode 1.14.46／1.18.33 或 Electron 桌面；fixture 回归不等同于桌面实测。
建议测试机器人复测两次工具允许、拒绝后继续、重启恢复，以及已有错序会话首次打开。
此前报告的“未发送草稿刷新后丢失”尚未确认归属，本修复不包含该问题。

## Test coverage

在 macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.7 下测量完整 workspace，默认 features、
默认文件过滤，无额外排除，不插桩 doctest。测量源码为上述基线加
[源码指纹与覆盖率证据](opencode-tool-order-coverage.json)，随后只修改报告。

| 范围             | 行覆盖率 | covered / total |
| ---------------- | -------- | --------------- |
| Workspace        | 94.50%   | 49,132 / 51,991 |
| provider         | 93.96%   | 21,409 / 22,786 |
| OpenCode adapter | 86.89%   | 2,671 / 3,074   |

OpenCode 相比 `4840a3a0` 的同平台、同编译器和工具版本历史测量 86.53% 提高 0.36 个百分点；
基线未重新测量。Workspace/provider 因本次同步包含上游无关源码与测试变更，不作直接增减比较。
JSON 提供逐文件指标、完整源码 SHA-256 和执行日志摘要；HTML 生成于
`target/llvm-cov/html/index.html`。HTML 留在本地，PR 中的 JSON 是共享审阅证据。

执行结果单独记录：

- OpenCode 定向回归：49 passed，另补充历史延迟写入测试 1 passed。
- 最终源码 `cargo test --workspace`：1,776 passed、0 failed、3 ignored。
- workspace 覆盖率测试：1,776 passed、0 failed、3 ignored。
- 完整格式、Clippy、构建、文档 Oxfmt、文档链接检查及 `git diff --check` 通过。
- 3 项 ignored 沿用已安装 Claude 发现、真实 Claude 和 Codex 在线回合；没有新增跳过。

提交前命令（均在 `nix develop --command` 内执行）：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline -- --test-threads=4
cargo llvm-cov --workspace --locked --offline --html -- --test-threads=4
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-opencode-tool-order-coverage.json
```

未覆盖部分包括真实 OpenCode 进程异常关闭、部分推理、限制和畸形历史分支。
未在 Linux/Windows、原生移动端或 Electron GUI 上验证；真实版本桌面复测仍需测试机器人。
