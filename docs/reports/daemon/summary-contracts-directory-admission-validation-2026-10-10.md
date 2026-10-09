# 摘要契约与目录准入：提交验证

2026-10-10；源码提交 `c5dc75fd4a398038bbc5749d50edce94f265c761`，基于 main `b2dba057f90abc5f5f0fdd6b740a52fbaa5b31dd`。
测量树为 `229b18e65b82e732a41e8cf4d76f62d884fa4bec`；随后只增加验证报告和索引，不改变 Rust 源码。
边界决策见 [ADR-118](../../decisions/daemon/adr-118-summary-contracts-and-persistence-configuration.md)，
目录预算见 [ADR-037](../../decisions/daemon/adr-037-daemon-model-context.md)。

摘要生成与配置契约移入 model；provider 保留生成实现，persistence 的原草稿已接入编译，
daemon 注入配置适配器并删除重复实现。目录订阅读取使用独立串行预算，避免变更唤醒挤占
前台请求的 `jobs`；新预算字段私有，外部通过 `run_directory_read` 调用。

## 验证

- `cargo test --workspace -- --test-threads=4`：2002 通过、0 失败、14 忽略；doctest 阶段通过。
- 覆盖率完整测试：2002 通过、0 失败、14 忽略，不含 doctest。
- fmt、workspace build、严格 Clippy、文档本地链接、覆盖率辅助脚本 Python 语法和 diff 检查通过。
- 回归覆盖目录预算争用、前台准入、排队与停机取消；摘要适配器覆盖配置热更新、项目根、
  `ait.json`/`paseo.json` 回退及错误映射；完整测试覆盖生成器、API 适配、标题与 daemon 组装。

## Test coverage

Workspace 行覆盖率 **94.7087%（55880 / 59002）**，相对可比基线 **+0.0088 个百分点**。
model **96.2536%（1336 / 1388）**，persistence **95.8734%（1696 / 1769）**，
provider **94.2613%（26445 / 28055）**，API **94.4149%（2130 / 2256）**。
Runtime 生产文件 **97.4359%（114 / 117）**；摘要配置适配器生产文件 **100.0000%（19 / 19）**。

命令：`cargo llvm-cov --workspace --html -- --test-threads=1`。
范围：全部 13 个 workspace package、默认 features、macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4，
无额外文件排除；14 项原生 Provider 用例按默认配置忽略。Doctest、shell/Python fixture
和上游二进制不在 Rust instrumentation 范围，Linux/Windows 未纳入本地测量。

基线源码 `fa5fc947a95a18bf0a95b606c8312b172e875d1f` 的 Rust 指纹与最新 main 一致，测量命令、特性、平台与
忽略测试范围相同。基线 workspace 为 94.6999%（55854 / 58980），
详见 [基线证据](crate-visibility-pr-coverage-2026-10-09.json)。
[当前共享证据](summary-contracts-directory-admission-coverage-2026-10-10.json)包含精确命令、源码哈希、
逐 crate 计数、改动文件覆盖率和 LCOV 未覆盖行。本地 HTML 为 `target/llvm-cov/html/index.html`。

| Package | 行覆盖率 | 相对基线 |
| --- | --- | --- |
| api | 94.4149%（2130 / 2256） | +0.0000 pp |
| browser | 97.5443%（715 / 733） | +0.0000 pp |
| daemon | 94.7520%（1318 / 1391） | -0.0597 pp |
| domain | 100.0000%（549 / 549） | +0.0000 pp |
| filesystem | 94.9247%（11596 / 12216） | +0.0082 pp |
| metadata | 94.1562%（5462 / 5801） | +0.0142 pp |
| model | 96.2536%（1336 / 1388） | +0.0603 pp |
| persistence | 95.8734%（1696 / 1769） | +0.1591 pp |
| provider | 94.2613%（26445 / 28055） | +0.0000 pp |
| relay | 91.8489%（462 / 503） | +0.0000 pp |
| schedule | 97.5980%（772 / 791） | +0.0000 pp |
| terminal | 96.4713%（1531 / 1587） | +0.0000 pp |
| voice | 95.1605%（1868 / 1963） | +0.0000 pp |

Runtime 取得许可后、进入任务准入前的停机竞态仍未覆盖，与基线相同；后续修改准入逻辑时补充确定性竞态注入。
部分原生 ACP 配置/取消回调与协议、进程 I/O 错误仍有覆盖缺口；后续修改对应路径时补充
通知回归和故障注入，并在 Linux/Windows 及隔离原生 Provider 环境复查。
此前摘要草稿已接入并测量；重号的旧摘要 ADR-109 由 ADR-118 取代。
