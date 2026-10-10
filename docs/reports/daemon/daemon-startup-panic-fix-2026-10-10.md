# 本机 daemon 终端 panic 与启动失败修复

- 日期：2026-10-10，Asia/Shanghai。
- 源码基线：`d8a5fb8e72a4d1dfd370aaf4975055e438881d9f`，Ait 0.0.25。
- 修复源码提交：`0b4897849dd412df1674f3e31649e8cd1398c065`，分支 `codex/fix-daemon-runtime`；随后只增加文档。
- 平台：macOS arm64，Rust 1.98.1，默认 features。

## 原因与修改

实际日志没有显示 daemon 进程持续退出，但记录了三次 `terminal-output` 线程 panic：
`vt100-0.16.2/src/screen.rs:870` 的 `Option::unwrap()` 失败。宽字符在缩窄窗口时被裁掉
第二格，剩余第一格仍标记为宽字符；再次写入会访问已不存在的格子。

新增测试先在原依赖上复现相同文件和行号。依赖锁定为上游
[PR #30](https://github.com/doy/vt100-rust/pull/30)的提交
`f2a4d76f9a663894d7bd56355caf150c106556ca`，修复宽字符裁剪及一行／一列终端的相关越界。
测试覆盖裁剪后写入、清行、截图恢复和单格输出。该 PR 尚未进入正式版本，后续包含修复的
正式 release 可替换当前固定 Git 依赖。

OpenCode ACP discovery 对没有思考选项的模型生成 `defaultThinkingOptionId: null`，
而客户端只接受可选字符串，导致整份 Provider snapshot 和主机诊断校验失败。
现在没有默认值时省略该字段；夹具覆盖同一目录中有／无思考选项的两个模型。

`connection.single.v1` 的各能力组只有两个入站槽位，正常启动突发会被拒绝。原版在空
数据目录同时发送 30 个只读请求得到 9 个响应、21 个 `resource_exhausted`；修复版 30 个
全部成功。每组改为有界 16 槽，超量 RPC 仍拒绝，Ping 不等待业务 worker。
预算决策及内存影响见 [ADR-120](../../decisions/daemon/adr-120-single-connection-startup-admission.md)。

## 测试执行

本机排查阶段运行改动和直接相关路径：

| 命令                                                                                                                              | 结果                                                    |
| --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `cargo test -p terminal screen::tests --lib`                                                                                      | 26 通过                                                 |
| `cargo test -p terminal local::tests --lib`                                                                                       | 13 通过；1 项因 sandbox 禁止 `ps` 失败                  |
| `cargo test -p terminal local::tests::kill_terminates_background_group_after_shell_has_already_exited --lib`                      | 授权环境重跑，1 通过                                    |
| `cargo test -p provider local::opencode::tests --lib`                                                                             | 22 通过，5 项原生安装验收按默认配置忽略                 |
| `cargo test -p api tests::single --lib`                                                                                           | 2 通过                                                  |
| `cargo test -p api tests::chunks --lib`                                                                                           | 4 通过，含忙碌 worker 后的 10 项正常突发、32 项超量拒绝 |
| `cargo test -p daemon --test process unix::terminal::terminal_methods_stream_frames_and_connection_owned_release_work_end_to_end` | 1 通过                                                  |
| `cargo clippy -p api -p terminal -p provider --all-targets -- -D warnings`                                                        | 通过                                                    |
| `cargo fmt --all --check`                                                                                                         | 通过                                                    |
| `cargo build -p daemon --bin daemon --release`                                                                                    | 通过                                                    |

共验证 69 个不同的定向测试。需要 PTY、本机端口或进程检查的测试在授权环境执行。
5 项忽略用例需要 OpenCode 真实安装及隔离模型服务，未将其计入通过数。

提交准备阶段已完成完整 workspace 检查：

| 命令                                                             | 结果                                          |
| ---------------------------------------------------------------- | --------------------------------------------- |
| `cargo fmt --all --check`                                        | 通过                                          |
| `cargo build --workspace --locked`                               | 通过，0 warning                               |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过，0 warning                               |
| `cargo test --workspace --locked -- --test-threads=4`            | 2,014 通过，0 失败，14 忽略；doctest 阶段通过 |

完整测试使用默认 features；14 项忽略测试沿用原有标记，涉及 AGY、Claude、Codex、DSH 与
OpenCode 的真实安装、认证、既有原生会话或模型验收。没有新增忽略标记。

## 本机安装与检查

原 0.0.25 应用完整备份到仓库忽略目录 `.tmp/daemon-hotfix-20261010/original/Ait.app`。
将最终 release daemon 放入应用副本，用原安装包同一 Developer ID 签名，安装到
`/Applications/Ait.app` 并重新启动。系统签名校验
`codesign --verify --deep --strict /Applications/Ait.app` 在授权环境通过。
用户数据目录保留，版本号仍为 0.0.25；此为本机修复，尚未发布安装包。

应用导出的修复后诊断显示本机 `online`、Agent directory `ready`、daemon lifecycle
`ready`，5 个 Provider 均可用，先前的默认思考选项字段校验错误消失。
最终启动时仍有 66 条可重试资源告警，集中于最初约 4 秒；随后数分钟没有新告警、panic
或 daemon 重启。测试证明正常突发可进入队列，并不宣称所有预算竞争的资源错误都已消失。
后续若出现持续资源拒绝，需要定位具体业务操作和运行预算，不能将告警直接等同于进程崩溃。

原始诊断与用户数据快照仅用于本机排查，未加入版本控制。本报告记录脱离私有会话内容的
复现、命令及结果。

## Test coverage

**Not measured**：提交准备时已启动
`cargo llvm-cov --workspace --html --locked -- --test-threads=1`，在插桩构建完成、测试执行期间，
用户明确要求“跳过这些测试，直接推送吧”，因此停止剩余覆盖率运行并继续提交。
当前修复没有完整的行覆盖率百分比、covered/total 行计数或可分享的覆盖率 artifact；
没有当前测量可用于基线比较。上述通过数仅表示普通测试执行结果。

拟测量范围为上述源码提交的全部 13 个 Cargo workspace package、默认 features、macOS arm64、
Rust 1.98.1、cargo-llvm-cov 0.8.4，使用默认源码过滤，无额外文件排除；14 项原生 Provider
验收用例按默认标记忽略。Doctest、Python fixture 和外部 vt100 依赖不在 workspace
instrumentation 范围；Linux/Windows 和长时间运行尚未测量。

新增宽字符裁剪、单格终端、可选思考字段及正常／超量请求突发已有回归用例，完整普通测试通过。
后续需要完整运行上述覆盖率命令并保存可共享产物，在隔离的原生 Provider 环境及其他平台复查；
本机几分钟观察只证明该观察窗口内的状态。
