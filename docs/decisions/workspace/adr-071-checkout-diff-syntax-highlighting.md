# ADR-071：在 Rust checkout adapter 生成 Diff 语法 token

状态：Accepted（2026-10-02）

语言选择、Diff 配色和完整快照预算策略由
[ADR-137](adr-137-github-style-diff-highlighting.md) 修订（2026-10-11）。

## 背景

Diff 画布已经根据 line `tokens` 和客户端语法主题着色，但 Rust checkout RPC 一直把
`tokens` 置为 `None`。因此工作区和提交历史 Diff 均只有增删背景，没有代码语法颜色。

对照本地 Paseo `b5b43edd65cc1253493b13cca3941dd390df6ef3` 的
`packages/server/src/server/utils/diff-highlighter.ts` 和
`packages/server/src/utils/checkout-git.ts`：上游在服务端读取比较两侧完整文件，分别解析，
按 old/new 行号填回 token；完整内容不可用时按 hunk 行号重建文本。

## 决策

- 在 `filesystem::local::checkout` adapter 执行相同的快照选择和行号映射流程。
  uncommitted 比较 HEAD 与工作树；base 比较 merge-base 与 HEAD；commit 比较 first parent
  与指定 commit。rename 的旧版本使用 `old_path`；新增、删除与初次提交允许一侧缺失。
- checkout port 的 `DiffLine` 携带可选、与主题无关的 `HighlightToken` 值；application 继续
  协调用例，RPC 只把 port token 投影到既有协议字段，不读取文件、不依赖解析器。
- Rust 实现使用 `two-face` 的 syntax definitions 和其 `syntect` re-export，采用纯 Rust
  `fancy-regex` 后端。Paseo 使用 Lezer；Ait 将 syntax scopes 映射到现有客户端的
  `keyword`、`comment`、`string`、`number` 等语法角色，客户端决定颜色。
- 缺失或超过预算的完整文件回退到 hunk 重建；保持真实行号间的空行，不拼接不同位置。
  token 文本必须与对应 Diff 行完全一致，防止并发文件修改或忽略空白时错位。
- 沿用 Paseo 的 10,000 字符单行保护，完整文件和重建文本各限 1 MiB。解析失败、未知
  语言和病态长行保留普通 Diff。工作树读取只允许仓库内普通文件。
- 每个 checkout adapter 共享内容摘要与语法名称缓存，最多 32 项、8 MiB。语法定义只加载
  一次；Git 内容按仓库、路径和不可变 commit SHA 缓存，避免每次轮询重复 `git show`；
  工作树按内容摘要失效，缓存不依赖客户端主题。解析仍在既有 blocking-job 边界执行。
- RPC 按实际 JSON 编码字节检查单文件和完整订阅快照，接近既有 4 MiB 输出队列预算时
  去掉可选 token，优先保留 Diff 文本。纯文本单文件仍超预算时，保留文件名、增删统计，
  清空 hunks 并返回既有 `too_large` 状态；完整快照仍超预算时，清空文件内容并返回既有
  `diffTooLarge` 提示。完整快照检查包含 cwd，预留 64 KiB 给协议信封和其他在途消息。
  初次请求、订阅更新和提交文件 Diff 复用同一投影，避免超限消息进入输出队列并断开连接。

## 后果

已有 Desktop、Web 和 Native Diff 画布直接获得语法颜色，无需前端补做解析或改变协议。
具体 token 边界和语言范围可能与 Lezer 不同；无法读取完整文件时，重建内容不能恢复 hunk
以外的多行语法上下文。去掉高亮后能满足预算的 Diff 继续展示普通文本；仍超限时显示
既有的大文件或大快照提示，连接和订阅保持可用，缩小 Diff 后恢复内容。

验证结果与 Test coverage 见[修复报告](../../reports/workspace/diff-syntax-highlighting.md)。
2026-10-03 的纯文本输出预算补充见[大 Diff 加载报告](../../reports/workspace/large-diff-loading.md)。
