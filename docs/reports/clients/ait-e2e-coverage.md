# Ait E2E 迁移：提交准备覆盖率报告

## Test coverage

测量代码版本：[`a933517`](https://github.com/necokeine/ait/commit/a933517fe726f11e37eb8e38898e46b50daef580)，基于 `c03c42d`。此测量对应该提交，不代表随后同步 main 的新代码已包含在同一份覆盖率中。测量日期：2026-09-29。没有同范围、同版本的可比基线，不计算覆盖率增量。

范围为整个 Cargo workspace，默认 features，debug profile，macOS `aarch64-apple-darwin`；Rust 1.98.1、cargo-llvm-cov 0.8.4。未传入自定义文件排除参数，采用工具默认过滤；未开启 doctest coverage。Windows、Linux、移动端及真实在线 Provider 未在本次测量中运行。三个需要本地 Claude/Codex CLI 或认证的 Rust 测试按默认设置忽略。

执行命令：

```sh
RUST_TEST_THREADS=4 cargo llvm-cov --workspace --html
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-pr-coverage-summary.json
cargo llvm-cov report --show-missing-lines
```

本文件是随 PR 提供的可审阅覆盖率摘要，以下计数直接来自同一次 LLVM coverage 数据。完整 HTML 已生成于 `target/llvm-cov/html/index.html`，未作为共享 HTML 上传；可用上述命令重建。原始 profile 和运行产物不提交。

| 范围                | 已覆盖 / 总行数 | 行覆盖率 |
| ------------------- | --------------: | -------: |
| Workspace           | 35,744 / 39,021 |   91.60% |
| `bins/daemon`       |       838 / 887 |   94.48% |
| `crates/api`        |   1,110 / 1,156 |   96.02% |
| `crates/browser`    |       707 / 726 |   97.38% |
| `crates/domain`     |       121 / 121 |  100.00% |
| `crates/filesystem` |   7,881 / 8,941 |   88.14% |
| `crates/metadata`   |   6,178 / 6,878 |   89.82% |
| `crates/model`      |       296 / 310 |   95.48% |
| `crates/protocol`   |         57 / 57 |  100.00% |
| `crates/provider`   | 15,247 / 16,414 |   92.89% |
| `crates/schedule`   |       802 / 820 |   97.80% |
| `crates/terminal`   |   1,289 / 1,436 |   89.76% |
| `crates/voice`      |   1,218 / 1,275 |   95.53% |

### 本次修改的可执行 Rust 源文件

文件名和计数保留测量时的历史路径；已迁移文件的链接指向当前定义，本报告未重新测量覆盖率。

| 文件                                                                                                                  | 已覆盖 / 总行数 | 行覆盖率 |
| --------------------------------------------------------------------------------------------------------------------- | --------------: | -------: |
| [crates/filesystem/src/local/files/search.rs](../../../crates/filesystem/src/files/local/files/search.rs)                   |       184 / 193 |   95.34% |
| [crates/filesystem/src/local/github_projects.rs](../../../crates/filesystem/src/forge/local/github_projects.rs)             |       221 / 245 |   90.20% |
| [crates/filesystem/src/local/worktrees.rs](../../../crates/filesystem/src/worktrees/local/worktrees.rs)                         |       670 / 756 |   88.62% |
| [crates/metadata/src/local/workspace_automation.rs](../../../crates/metadata/src/local/workspace_automation.rs)       |       473 / 537 |   88.08% |
| [crates/metadata/src/protocol/project_config.rs](../../../crates/metadata/src/protocol/project_config.rs)             |       137 / 156 |   87.82% |
| [crates/metadata/src/protocol/workspace_automation.rs](../../../crates/metadata/src/protocol/workspace_automation.rs) |           3 / 3 |  100.00% |
| [crates/metadata/src/service/directory.rs](../../../crates/metadata/src/service/directory.rs)                         |       712 / 778 |   91.52% |
| [crates/metadata/src/storage/project_config.rs](../../../crates/persistence/src/storage/project_config.rs)               |         79 / 84 |   94.05% |

仅改文档/类型声明而没有可执行行的文件不产生 coverage 记录。下面保留 `--show-missing-lines` 对这些文件列出的未覆盖行，便于按上述代码版本审阅：

- `crates/filesystem/src/local/files/search.rs`：44, 51, 182, 188, 193, 196, 204, 211, 240。
- `crates/filesystem/src/local/github_projects.rs`：27, 28, 29, 126, 172, 173, 175, 177, 185, 187, 194, 195, 196, 199, 204, 205, 206, 207, 208, 209, 234。
- `crates/filesystem/src/local/worktrees.rs`：46, 53, 59, 60, 105, 136, 191, 194, 218, 221, 222, 223, 224, 268, 269, 270, 271, 303, 305, 306, 348, 349, 350, 422, 423, 424, 428, 455, 477, 479, 485, 486, 487, 488, 489, 491, 492, 493, 570, 594, 605, 628, 629, 630, 657, 700, 742, 744, 745, 749, 759, 830, 831, 832, 862, 863, 864, 876, 877, 878, 883。
- `crates/metadata/src/local/workspace_automation.rs`：80, 154, 155, 185, 186, 211, 212, 213, 214, 215, 217, 232, 233, 234, 235, 236, 237, 239, 285, 298, 299, 300, 301, 331, 394, 395, 410, 411, 611, 612, 613, 614, 618, 619, 620, 622, 624, 638, 639, 640, 641, 642, 643, 644, 668, 669, 670, 671, 672, 673, 674, 675。
- `crates/metadata/src/protocol/project_config.rs`：33, 34, 35, 173, 174, 175, 176, 177, 178, 252, 274, 294, 305, 309, 318, 327, 338, 344, 345。
- `crates/metadata/src/service/directory.rs`：252, 265, 266, 267, 268, 269, 477, 491, 694, 695, 696, 699, 757, 763, 764, 765, 770, 771, 772, 776, 777, 779, 781, 782, 784, 788, 790, 791, 795, 796, 797, 799, 800, 801, 803, 804, 805, 806, 808, 817, 818, 819, 820, 821, 822, 925, 1010, 1057, 1091, 1092, 1093, 1094, 1103, 1104, 1105, 1106, 1107, 1108, 1110。
- `crates/metadata/src/storage/project_config.rs`：29, 89, 97, 100。

### 重要未覆盖行为与后续验证

- `project_config::read_path` 的非 NotFound 文件元数据错误分支（第 89 行）未覆盖；配置优先级、缺失回退、格式错误不回退和保存迁移已由测试覆盖。后续可通过文件系统错误注入验证权限/I/O 错误。
- worktree 配置复制仍有底层文件元数据和目标创建失败分支未覆盖（第 594、605 行）。已验证旧配置迁移、目标配置保护和常规工作树操作；文件系统故障注入可补齐这些异常路径。
- 继承的文件搜索、GitHub 克隆和自动化模块还有取消、子进程、系统错误分支未覆盖；不能从本次行覆盖率推导所有外部服务或平台行为已验证。
- TypeScript、浏览器 E2E 和原生 UI 未测量行覆盖率。继承的 browser 用例仍有旧 mock Provider、订阅/回执和 wire-frame 断言，需逐项迁移；本次只对六项核心 Ait E2E 声明执行通过。

## 测试执行结果

- `RUST_TEST_THREADS=4 cargo test --workspace`：1,271 通过，3 忽略，0 失败；coverage 执行同样通过。
- `cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`：通过。
- App 修改相关单测：30 文件 / 511 通过；Rust adapter、i18n 和 native release 检查：5 文件 / 73 通过。
- protocol：65 文件 / 734 通过；desktop：53 文件 / 373 通过、13 跳过。
- SDK transport：10 通过；连接/认证/重连筛选：14 通过、124 未选中。
- App、desktop、client 类型检查；桌面与依赖构建；发布检查和 12 项发布脚本测试：通过。
- 145 个修改的 TypeScript/JavaScript 文件：`oxfmt --check`、`oxlint -A no-empty-pattern` 通过，无警告。
- Ait 认证、浏览器票据、离线 Codex 对话、配置迁移、服务重启和进程重启恢复：6 项 E2E 通过。命令及边界见[迁移报告](ait-e2e-migration.md)。
