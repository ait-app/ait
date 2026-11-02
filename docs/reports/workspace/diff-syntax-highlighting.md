# Diff 代码语法高亮修复

日期：2026-10-02。基线：`e098e9792699a8231c56ca0a8332e86d4007796f` 加当前未提交改动。
架构决策：[ADR-071](../../decisions/workspace/adr-071-checkout-diff-syntax-highlighting.md)。

以下为初始定向验证记录。与最新 main 合并后的完整 PR 检查和当前覆盖率见
[提交验证报告](diff-pr-validation.md)。

## 原因与 Paseo 对照

前端 `git/diff-document/model.ts` 从 line `tokens` 构造颜色区间，Web/Native 画布已经支持
语法颜色。Rust `rpc/checkout.rs::protocol_diff_file` 原先却为所有行固定设置 `tokens: None`。
[第八阶段移植报告](../../archive/reports/daemon/paseo-websocket-surface-phase-8.md)也记录了这个尚未补齐的行为。

对照本地 Paseo `b5b43edd65cc1253493b13cca3941dd390df6ef3` 的
`packages/server/src/server/utils/diff-highlighter.ts`、
`packages/server/src/utils/checkout-git.ts` 和对应测试，原版在服务端读取完整旧、新文件，
分别高亮，再按 hunk 行号映射 token。删除、缺失内容等情况回退到按真实行号重建文本。

## 修复

Rust checkout adapter 现在生成与主题无关的语法角色；RPC 保留这些 token，前端继续按
用户主题着色。工作区比较 HEAD 与工作树，base 比较 merge-base 与 HEAD，commit 比较
first parent 与指定 commit；rename 的旧文件使用 `old_path`。完整文件提供 hunk 外的多行
注释和字符串上下文，无法读取时重建 hunk 文本。token 必须完整还原对应行，防止错位。

解析实现使用 [two-face](https://docs.rs/two-face/0.5.2%2Bbat-0.26.1/two_face/) 提供的语法定义和
[syntect](https://docs.rs/syntect/5.3.0/syntect/parsing/struct.ParseState.html) 的跨行解析状态。
这是 Rust 实现；Paseo 的 Lezer 解析器与它的具体 token 边界、语言覆盖并不完全相同。

解析限制为单行 10,000 字符、完整或重建文本 1 MiB；语法缓存最多 32 项、8 MiB。Git
内容使用不可变 commit SHA 缓存，工作树使用内容摘要缓存；变化时重新解析。单文件和
完整订阅快照经过实际 JSON 字节预算检查，高亮导致超限时省略 token 并保留 Diff 文本。

## 假文件项回归修复

新增高亮测试包含 `parse_diff(&format!("diff --git a/{path} b/{path}\n{body}"))`。
旧解析器在任意位置按 `diff --git ` 切分，因此截断源代码行，并把剩余的
`{path}\n{body}` 误识别为无 hunk 的文件名，界面显示 `+0 -0`。

Paseo 的 Diff 高亮解析和多文件 Git 输出拆分都使用行首匹配。Rust 现在也只在输入开头或
换行之后接受 Git 文件头，代码行中的字符串和注释保留在原文件内。
新增四项回归覆盖新增、删除、上下文行内的假头，后续真实文件、无真实头输入，以及
CRLF 和缺少末尾换行。修复前其中三项失败，修复后四项全部通过。

## 验证

- `cargo test --offline -p filesystem local::checkout::tests::parser --target-dir /private/tmp/ait-diff-highlight-target`：
  4 passed、0 failed。
- 下述 `cargo llvm-cov` 的 `checkout` 定向运行：111 passed、0 failed，包括高亮、解析器、
  真实 Git、RPC、协议和订阅相关回归。
- App 的 `model.test.ts`、`paint.web.test.ts`、`paint.native.test.ts`：30 passed、0 failed。
- 新增 17 项高亮回归覆盖常用语言、旧/新/上下文行、多行语法、模板插值、rename、删除、
  untracked、base/commit 快照、HEAD 与工作树变化、缓存、长行/稀疏 hunk 和输出预算。
- 修改 Rust 文件的定向 rustfmt、文档 Oxfmt 和 `git diff --check` 通过。
- 根 Cargo workspace 的 `cargo check --offline --workspace` 通过；设置
  `SHERPA_ONNX_LIB_DIR` 使用已缓存的 Sherpa ONNX 静态库，target 为
  `/private/tmp/ait-diff-highlight-lint`。首次尝试的语音库下载受 sandbox DNS 限制，使用现有
  静态库后完成检查。
- `cargo clippy --offline -p filesystem --all-targets --target-dir /private/tmp/ait-diff-highlight-lint -- -D warnings`：
  通过；先前其他未提交改动造成的 lint 阻挡已消失，本次未为其添加 lint allow。
- 未运行 live Desktop UI 或 iOS/Android 设备验收；运行中的旧 server 需要重建、重启才能
  使用新逻辑。本次未执行提交、打包或发布。

## Test coverage

使用 `cargo llvm-cov` 在 macOS arm64、默认 features、当前工作树上执行 `checkout` 定向测试，
111 passed、0 failed、243 filtered out。新模块 `local/checkout/highlight.rs` 的行覆盖率为
**99.36%（312/314）**；`local/checkout.rs` 为 85.49%（1,255/1,468），
`rpc/checkout.rs` 为 82.41%（445/540）。未注入 parser/scope 操作失败或缓存锁中毒故障。

该定向运行在整个 `filesystem` crate 的计数为 35.40%（4,009/11,326），不是完整 crate
测试覆盖率。没有可比基线，不计算覆盖率增减；工作区全量测试和 coverage 延后到提交准备。
报告使用 llvm-cov 默认文件过滤；测试函数本身不计入生产代码覆盖率。

精确命令、文件 SHA-256、范围和计数见[可审阅 coverage 摘要](diff-syntax-highlighting-coverage.json)。
原始 JSON 为 `/private/tmp/ait-diff-highlight-coverage.json`，复现命令：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-diff-highlight-coverage cargo llvm-cov test \
  --offline -p filesystem --lib checkout --json \
  --output-path /private/tmp/ait-diff-highlight-coverage.json
```
