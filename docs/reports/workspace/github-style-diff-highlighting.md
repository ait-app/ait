# Diff 的 GitHub 风格高亮验证

日期：2026-10-11。基线：`3353efe379fdd7ebaba6aa9eabe17fb5fcf2ffa2` 加本次 PR 改动。
决策：[ADR-137](../../decisions/workspace/adr-137-github-style-diff-highlighting.md)。

## 改动与可见行为

Diff 配色继续跟随用户选择的全局语法主题，GitHub 和 One 等已有主题均按选择生效。
Rust 分别选择旧、新文件的语言；新增 `.gitattributes` 的 `linguist-language`、文件前后五行
Vim/Emacs modeline、完整文件名和 shebang 提示。旧版本读取旧树 attributes，工作树与
base/commit 比较读取各自目标侧，避免未提交 attributes 改动污染历史高亮。

单文件检查继续保留大文件普通文本或 `too_large` 提示。完整快照超预算时，按真实 JSON
字节数优先处理昂贵文件：文本超预算时仅清除必要文件的 hunks；高亮超预算时仅清除
必要文件的 tokens。文件顺序、名字和增删统计保留，小文件尽量继续显示代码和语法色。
只在元数据仍超预算等兜底情况下清空完整文件列表。

预算计算为线性 JSON 计数与两次按文件排序，不为计数分配 JSON 字节副本，也不在每次
清除一个文件后重编码完整响应。语法仍由 `two-face / syntect` 提供，不声称完整复制
GitHub 的 Linguist 语言库、歧义语言识别、token 边界或大小限制。

## 验证

修复前，新增的 `aggregate_budget_preserves_small_file_highlights_regardless_of_file_order`
失败：两份大文件使一行小文件的 token 被整体清除。修复后通过，并覆盖小文件在数组
开头和末尾的情况。

- 当前定向 Rust 回归：高亮/语言检测 13 项、真实 Git 高亮 8 项、checkout RPC 18 项、
  JSON 计数预算 2 项，共 41 项通过。包含跨行语法、缓存、rename、历史 attributes、
  未提交 attributes 更新、未知语言、超长文本、必要文件降级、订阅去重与恢复。
- 较早的 `checkout` 名称过滤回归：156 项通过；没有执行全 Rust workspace 测试。
- 客户端 palette、model、Web/Native paint 和 Diff sharing：5 个文件、65 项通过。
  palette 回归覆盖 One/GitHub 所选语法主题与明暗模式；本次恢复主题跟随行为只涉及
  TypeScript 与文档，按仓库规则没有重跑 Rust 测试。
- `cargo clippy --offline -p filesystem --all-targets -- -D warnings` 通过。
- `cargo check --offline --workspace` 通过。
- App 的 `npx tsgo --noEmit` 通过；补齐浏览器 seed client 已使用的
  `getCheckoutDiff` 类型声明，直接复用 SDK 的方法类型。
- 变更 Rust 文件的 rustfmt、TypeScript/文档 Oxfmt、TypeScript Oxlint 与
  `git diff --check` 通过。
- 浏览器两个用例通过：单文件超限时，小 Rust 文件的代码仍可见，Canvas 实际绘制的
  关键字色符合所选语法主题；完整快照跨限时保留文件列表和普通文件，跳转到
  受限文件能看到提示，缩小 Diff 后恢复，WebSocket 连接数与关闭数不变。
  恢复所选语法主题后，单文件用例设置 One 浅色主题并重新验证实际绘制的关键字色，
  通过（4.8 秒；含隔离测试服务构建和启动共 25.9 秒）。完整快照恢复用例在
  `2113a502` 上通过（6.1 秒），本次未修改该用例的运行行为。
- 浏览器初次尝试被沙箱临时端口限制阻挡，获得隔离测试运行权限后继续。原有用例的
  Explorer 标签选择器已过时，改为当前 Diff tab 入口；原有 75,000 行样本在当前 JSON
  格式下未跨限，增加为每文件 100,000 行。受限文件可能排在仍有大量内容的文件后面，
  验证提示前先通过文件树定位实际带 `too_large` 状态的文件，避免视口外断言。

复现命令：

```sh
cargo test --offline -p filesystem --lib git::local::checkout::highlight::
cargo test --offline -p filesystem --lib git::local::checkout::tests::highlight::
cargo test --offline -p filesystem --lib git::rpc::tests::
cargo test --offline -p filesystem --lib support::budget::tests
cargo clippy --offline -p filesystem --all-targets -- -D warnings
cargo check --offline --workspace
```

在 `apps/mobile`：

```sh
npx vitest run --project unit src/git/diff-document/palette.test.ts \
  src/git/diff-document/model.test.ts src/git/diff-document/paint.web.test.ts \
  src/git/diff-document/paint.native.test.ts src/git/diff-sharing.test.ts
npx playwright test --project=browser e2e/browser/diff-size-limit.spec.ts
npx playwright test --project=browser e2e/browser/diff-size-limit.spec.ts \
  --grep 'an oversized file'
npx playwright test --project=browser e2e/browser/diff-size-limit.spec.ts \
  --grep 'oversized snapshots recover'
npx tsgo --noEmit
```

## Test coverage

**Not measured**：用户没有请求覆盖率测量。本次只执行变更及相关行为的定向测试，
测试通过数不表示覆盖率；若后续需要，可对上述 Rust filters 使用 `cargo llvm-cov`
生成可审阅产物。没有完整 workspace 覆盖率、Native 设备验收或 GitHub grammar 等价性
验证。实现和验证记录可在上述基线加 PR diff 中审阅。
