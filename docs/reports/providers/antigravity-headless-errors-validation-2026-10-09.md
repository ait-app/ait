# AGY headless 拒绝与错误诊断：PR 验证

日期：2026-10-09。源码提交：`23d332cc327c416d2947d6f6d9333bdf27f95df7`；base：`fdd6ce6375b9f40c9f5d5eb22216f11e238bf736`。

AGY 1.3.0 在 headless 中自动拒绝需要审批的命令时，可能返回退出码 0、`SUCCESS`、空响应和 `denied_actions`，工具 DONE 事件不含输出或错误。本次修复将无响应的拒绝轮次和对应工具标为失败，把同一分类说明传递到会话记录、终止事件和活动日志。已有非空成功响应保留；正常无输出工具仍可完成。

stderr 持续、有界地读取，仅保存分类结果。测试覆盖管道分片、512 KiB 输出、原始诊断不进入会话记录、协议失败及取消行为。执行模式说明提示配置有范围的 AGY allow rule 或显式选择 Full Access。

## Validation

- 普通完整 workspace 测试：2032 通过、1 个既有目录订阅用例等待超时、15 默认忽略。随后整个 daemon 进程目标串行复跑：95 通过。合并唯一用例结果为 2033 通过（包含 1 个 doctest），这不是第二次普通完整 workspace 调用。
- 完整 instrumented workspace：2032 通过、0 失败、15 默认忽略；LLVM 不计入该 doctest。
- `cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。
- AGY protocol 测试：1 通过；修改的 TypeScript 文件通过 oxfmt、oxlint；文档链接检查与 `git diff --check` 通过。
- 首次完整运行的目录订阅超时、后续复跑结果，以及 main 诊断测试的 macOS `/bin/false` 路径错误均保留在 JSON 证据中。测试路径修正为 `/usr/bin/false`；未修改目录订阅逻辑。

## Test coverage

| Scope | Covered/total lines | Line coverage |
| --- | --- | --- |
| Workspace | 56928/60463 | 94.15% |
| provider | 27535/29540 | 93.21% |
| AGY production | 1109/1176 | 94.30% |

测量命令：`cargo llvm-cov --workspace --html --output-dir /private/tmp/ait-agy-pr-23zv638x/coverage -- --test-threads=4`。默认 Cargo features、默认 source filters、无额外文件排除；macOS 27.0.1 arm64，Rust 1.98.1，cargo-llvm-cov 0.8.4。构建与覆盖率使用独立 worktree 和 target 目录。

共享证据：[覆盖率 JSON](antigravity-headless-errors-coverage-2026-10-09.json) 包含源码指纹、文件 SHA-256、workspace 与各 crate 行计数、AGY 文件的未覆盖行和完整命令。HTML 另生成于测量输出目录；共享证据不依赖该本地路径。

历史默认测试参考：[OpenCode 覆盖率证据](opencode-private-protocol-coverage-2026-10-08.json) 的 default-only workspace 为 94.18%，provider 为 93.23%；本次分别变化 -0.025、-0.016 个百分点。两者默认 features、过滤与 Rust 版本相同，但历史 cargo-llvm-cov 为 0.8.7，源码也与本 PR base 不同；变化包含期间 main 改动，不能归因于 AGY 补丁。未单独测量本 PR 的精确 base。

限制与后续：

- 15 个默认忽略的 installed-provider 用例未执行；需要真实 CLI、认证或明确安装配置。真实 AGY 1.3.0 的拒绝形状在修复前用一次临时目录中的 `printf agy_diagnostic` 复现，修复后由离线 fixture 验证；未执行修复后的付费模型或 authenticated smoke。
- 进程启动/终止失败、stderr drain 超时、写入失败以及重连/初始化异常分支仍部分未覆盖；相关路径改变时补充确定性的故障 fixture。
- 既有目录订阅测试在普通完整运行中出现偶发 10 秒等待超时；单用例、串行完整进程目标和完整覆盖率运行通过。CI 若复现，需要单独调查订阅时序。
- 未验证 Linux、Windows 和桌面 UI 端到端行为；未测量 TypeScript 覆盖率、doctest instrumentation。行覆盖率不等于分支覆盖率。
