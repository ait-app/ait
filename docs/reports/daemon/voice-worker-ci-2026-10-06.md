# Voice worker CI fixture 验证

- 日期：2026-10-06；基线：PR [#186](https://github.com/ait-app/ait/pull/186) 的 `07c9afe51f667e14a82a7e05e3bdc1e8ef9a241a`。
- 实际上游失败：[CI 37432706092](https://github.com/ait-app/ait/actions/runs/37432706092)，Rust job `112167098816`。

## 已知事实与修复边界

上游 1832 项通过、1 项失败、6 项忽略。失败为
`invalid_worker_acknowledgments_drop_the_process_and_allow_a_retry` 的第二次 worker 启动返回
`Unavailable`。旧测试会在执行前创建并覆写可执行脚本；并发进程启动可能使可写描述符延长存活，
这是可避免的 fixture 风险，但原始 spawn errno 被丢弃，不能据此断言此次一定是 `ETXTBSY`。

修复让 voice 测试共用仓库内不可变的可执行 Python fixture，行为与启动计数仍各自写入临时目录。
新增 16 路并发测试覆盖无效 ACK、重试、正常复用、文件清理和实例隔离。既有断言与期限保留。
生产代码只为 spawn 失败增加 error kind / OS errno 日志，不输出路径、音频、请求或凭据，错误语义不变。

历史 directory 超时、一次本地 DSH `Unavailable` 与本次 voice 失败没有已证明的共同根因；
后续通过只说明这次执行成功，不能代替根因证明。桌面启动修复的 GUI、二进制 hash 与前端结果仍对应
`07c9afe`，见[独立启动报告](../clients/desktop-startup-isolation-2026-10-06.md)，本次没有重测 GUI。

## 检查

- `cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets -j1 -- -D warnings` 通过。
- 全 workspace coverage 执行：1833 通过、1 失败、6 忽略；voice 78 项与 provider 616 项全部通过。
- 唯一失败是已有 filesystem fixture 的 UnixListener bind 被本地执行环境以 `EPERM` 拒绝。
  未更改该断言或绕过限制。上游新提交的 CI 需独立验证。
- 不可变 Python fixture 的 160 组并发协议/重试隔离检查通过。

## Test coverage

Linux x86_64 / Rust 1.98.1 / cargo-llvm-cov 0.8.7；workspace default features、默认文件过滤，
不额外排除文件，doctests 未插桩。关闭 incremental 与 dev/test debuginfo。
本轮清空前次生成的原始 profile 后运行，没有混合旧源码的 profile。
命令：`cargo +1.98.1 llvm-cov --workspace --locked -j1 --html --no-clean --ignore-run-fail`。
`--ignore-run-fail` 允许收集报告，不表示整套测试成功。

| 范围                 | 已覆盖 / 总行数 | 行覆盖率 |
| -------------------- | --------------- | -------- |
| workspace            | 50958 / 54019   | 94.3335% |
| voice                | 1855 / 1951     | 95.0794% |
| voice offline worker | 158 / 172       | 91.8605% |

机器可读汇总见[JSON](voice-worker-ci-2026-10-06.json)；逐文件 HTML 保存在独立验证附件。
未覆盖部分包括部分 timeout/子进程回收异常分支；macOS、Windows 与真实语音模型未在本轮运行。
同平台前次 workspace 为 94.3420%，本次减少约 0.0085 个百分点；异步路径存在调度差异，
不把微小差值解释为某个测试的单独影响。
