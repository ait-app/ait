# OpenCode 加载速度验证

性能测量基于 `ecb3705d` 的工作区修改；本 PR 独立移植到 `a73c16ba` 的 main 上验证。
平台为 Linux x86_64，使用本机 OpenCode 1.18.4 和
官方包 OpenCode 2.0.32。修复决策见
[ADR-178](../../decisions/providers/adr-178-opencode-startup-probe-and-restoration.md)。

## 复现与修复

| 问题                                     | 证据                                                                             | 修改                                                                                 |
| ---------------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Catalog 先后启动两次 ACP                 | 可用性检查在 daemon 当前目录执行 `initialize`，发现时又在目标目录执行一次        | 可用性仅检查安装与版本，协议和能力仍由实际目录发现确认                               |
| 1.x 目录反复冷启动 CLI                   | 1.18.4 先初始化 ACP，再启动 models、agent list 和逐个 debug agent 进程           | 同一个 ACP 内部的认证只读 `/config/providers`、`/agent` 提供完整目录；不创建查询会话 |
| CLI 回退串行等待                         | 模型查询结束后才开始模式与元数据查询                                             | 模型与模式查询并行，模式元数据维持最多四个并发                                       |
| 冷恢复、配置重连和辅助创建仍按 30 秒失败 | `session/load`、重连的 `session/resume`、辅助 `session/new` 没有使用既有启动预算 | 统一使用 120 秒启动预算，普通控制和已有连接上的历史结算仍为 30 秒                    |
| 不知道等待发生在哪一阶段                 | 总耗时无法区分版本、gate 排队和初始化                                            | 分别记录固定阶段的调试耗时，不记录配置和认证信息                                     |

1.18 的只读目录沿用已存在的受管 server、loopback、每次启动独立认证、禁用系统代理及
重定向和有界响应。原生接口不可用或元数据无效时回退到 CLI；其他 1.x 版本仍走 CLI，
2.x 继续使用官方 ACP 的临时会话与删除能力。
没有关闭用户插件或 MCP，也没有改写用户配置、认证、数据库或已有会话。

## 加载耗时与范围

使用真实用户配置副本与原生 CLI 测量可用性、模型目录、创建和恢复。最终测量不发送
prompt，不等待模型 API；每轮使用新的隔离 XDG、项目和数据库，保留用户原生配置。

修复前的单次测量：

| OpenCode |  可用性 |     目录 |    创建 |    恢复 |
| -------- | ------: | -------: | ------: | ------: |
| 1.18.4   | 3.287 s | 12.445 s | 3.747 s | 3.760 s |
| 2.0.32   | 0.837 s |  1.565 s | 1.377 s | 1.550 s |

修复后首轮三次纯加载结果（单位：秒）：

| OpenCode | 可用性中位数 | 目录中位数 | 总加载中位数 | 三次总加载范围 | 创建中位数 | 恢复中位数 |
| -------- | -----------: | ---------: | -----------: | -------------: | ---------: | ---------: |
| 1.18.4   |        0.952 |      4.510 |        5.535 |    5.440–5.649 |      3.785 |      3.816 |
| 2.0.32   |        0.164 |      2.339 |        2.497 |    2.141–2.544 |      1.973 |      1.686 |

总加载为每轮“可用性＋目录”的和，再取三轮中位数，分别计算的阶段中位数不必相加相等。
1.x 从此前单次 15.732 秒降到三轮中位 5.535 秒；创建和恢复的原生启动成本基本保持原值。
2.x 此前单次 2.402 秒，本次三轮中位 2.497 秒，不能据此宣称整体已经稳定提速。

为隔离 2.x 重复探测，又使用同一个测量器恢复旧 ACP 可用性探测，保持目录路径不变：

| 2.x 对照     | 可用性中位数 | 总加载中位数 | 三次总加载范围 |
| ------------ | -----------: | -----------: | -------------: |
| 旧 ACP 探测  |        1.271 |        4.042 |    3.066–4.791 |
| 轻量版本探测 |        0.216 |        3.565 |    2.956–4.716 |

可用性探测的额外冷启动已消除；整段加载仍受原生目录初始化和本机负载影响，对照范围
明显重叠。因此 2.x 的结论是减少确定的重复工作，本轮不能保证所有配置的稳定总耗时收益。
这些测量没有关闭用户插件或 MCP，也没有固定或裁剪原生模型目录。

2026-10-21 曾将真实 API 验收与本地编译、多个原生测试同时执行，1.x 目录达到
28.467 秒；该组数据受本机负载影响，不作为速度对照。后续将加载测量与推理、并行
原生测试分离，聚焦 provider 本地加载路径。

可重复的加载测量默认 ignored，显式指定配置路径和已配置模型：

```sh
AIT_TEST_OPENCODE_BIN=/path/to/opencode \
AIT_TEST_OPENCODE_LIVE_CONFIG=/private/path/opencode.jsonc \
AIT_TEST_OPENCODE_LIVE_MODEL=provider/model \
cargo test --locked -p provider --lib live_profile_startup_without_inference -- --ignored --nocapture
```

每次执行测量三轮。设置 `AIT_TEST_OPENCODE_LIVE_ACP_PROBE=1` 可仅恢复旧的完整 ACP
可用性探测；2.x 的目录发现路径保持相同，可用于隔离额外冷启动的同机对照。
这不是 1.x 旧版目录实现的复原。若原生认证保存在单独文件中，可显式提供
`AIT_TEST_OPENCODE_LIVE_AUTH`，测试只复制到隔离目录。临时配置、日志、认证和数据库均不纳入 Git。
此前两版也已用真实 API 跑通工具、多轮、恢复和取消；模型服务等待不计入加载速度。

## 验证

离线回归使用协议屏障和虚拟时钟推进超过 30 秒，覆盖冷恢复、两条配置重连路径和辅助
创建，并检查请求数量、临时会话清理、权限和目录能力。原生版本测试使用隔离 loopback
模型验证真实进程和原生协议，不消耗外部 API 额度。

独立移植到 `a73c16ba` 后的定向验证（源码与当前 fork worktree 一致）：

| 范围                                                 | 结果                                         |
| ---------------------------------------------------- | -------------------------------------------- |
| OpenCode 单元与离线 ACP/HTTP 回归                    | 95 passed；27 项其他原生安装验收默认 ignored |
| Provider catalog                                     | 27 passed                                    |
| 真实 daemon 进程中的 OpenCode 发现、执行、恢复和导入 | 3 passed                                     |
| 真实 1.18.4 加载测量                                 | 3 轮通过，不执行推理                         |
| 真实 2.0.32 加载测量                                 | 3 轮通过，不执行推理                         |

```sh
cargo test --locked -p provider --lib local::opencode -- --skip live_profile
cargo test --locked -p provider --lib service::provider_catalog
cargo test --locked -p daemon --test process opencode
cargo clippy --locked -p provider --lib -- -D warnings
cargo fmt --all --check
npm run check:docs
git diff --check
```

`cargo build --locked --workspace` 通过。独立分支上的
`cargo clippy --locked --workspace --all-targets -- -D warnings` 被既有的
`clippy::assert_is_empty` 阻挡，位置为 `crates/host-link/src/controller/tests.rs:474`；
本轮未修改该断言。生产库严格 lint 单独验证。daemon 构建同时报告既有
`filesystem` 原子 `fetch_update` 弃用告警，OpenCode 修改没有新增构建告警。

本轮仅运行 OpenCode 与直接相关的 catalog、daemon 进程测试；没有运行完整 workspace 测试。

## Test coverage

**not measured**。用户请求加载速度探测与修复，没有请求覆盖率测量。本轮记录定向测试
和加载测量结果，不把通过数量换算为覆盖率，也不复用旧报告的百分比。
如后续请求测量，应使用 `cargo llvm-cov` 对 OpenCode 与直接相关 catalog 测试生成当前源码的报告。

未验证桌面 UI 的手动交互、其他操作系统和任意第三方插件组合。跨会话 ACP 共享、
附着外部后台 server 和自动替换不可用模型不属于本次变更。
