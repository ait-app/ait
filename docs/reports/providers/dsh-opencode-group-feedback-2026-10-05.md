# DSH 与 OpenCode 群反馈整合验证

日期：2026-10-05。基线为 `18631193`（upstream main / v0.0.19），在本地 `main` 整合；提交源码由随附制品中的 SHA-256 指纹标识。
DSH 来源为 `feat/dsh-native-host` 的 `f470cbda`，包括原生 Host、历史恢复、上下文过滤、缓存重复修复和 CI 测试隔离。
历史报告中的覆盖率和外部桌面结果只对应原提交，不作为本次验证结果。

## 问题与结果

| 群反馈 / 复现问题                        | 本次处理                                                                                                     |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| DSH 无法调整权限                         | 整合原生 Web Host adapter；支持 read-only、workspace-write、full-access，保留显式 ACP 兼容入口               |
| DSH 无法回答 question                    | 整合原生单选、多选和自由文本回答，以及共享表单数组答案支持                                                   |
| DSH CI 失败                              | 整合首次输入重试测试的会话隔离；原 PR #169 的最新远端检查已成功，本工作区相关测试重新验证                    |
| DSH 用户消息缺失、上下文混入和旧回复重复 | 整合原生历史修复和展示缓存版本迁移；保留真实同文消息，避免将重复加载的历史视为实时输出                       |
| OpenCode 经 Homebrew 升级到 2.x 后不可用 | 用官方 Homebrew 2.0.20 复现并修复以下三个协议差异                                                            |
| 模型发现 HTTP 400                        | `location` JSON 字符串改为嵌套查询键 `location[directory]`，完整编码目录中的空格、加号、& 和 Unicode         |
| 冷启动模型目录为空                       | 仅对合法空数组进行最多五秒的读取重试；不重试 HTTP 错误、畸形响应或非空但全部禁用的目录                       |
| 已回复但回合无法结束                     | 2.0.20 实测公开日志只返回 `log.synced`；以全量历史末尾的持久 idle 行判定完成，核对最近输入时间和会话 outcome |
| 共享插件行在分页边界重复                 | 补齐 stream 插件身份映射，沿用 protocol 的 pluginId/itemId 身份规则；原有三项失败回归通过                    |

OpenCode 原有版本门槛没有放宽；此前“2.0.0–2.0.9 被拒绝”只是静态观察，并非本次实测根因。
2.0.20 的 `OPENCODE_PASSWORD` 鉴权实测正常，没有按网上其他版本的行为改写此变量。
领域边界保持不变；DSH 沿用 [ADR-082](../../decisions/providers/adr-082-deepseek-harness-native-host.md)，
OpenCode 完成依据补充在 [ADR-074](../../decisions/providers/adr-074-opencode-native-provider.md)。

## 测试执行

平台为 macOS arm64，Nix Rust 1.98.1，默认 features。

| 范围                                                                                         | 本轮结果                                                                                             |
| -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| DSH 定向 provider 测试                                                                       | 31 passed，1 ignored；忽略的真实 CLI 测试另行执行通过                                                |
| 真实 DSH 0.1.5-rc.2                                                                          | 1 passed；隔离 DSH_HOME，模型发现、权限模式、关闭/恢复及旧 ACP 会话接续；不调用模型                  |
| OpenCode 定向 provider 测试                                                                  | 53 passed，1 ignored；忽略的真实 CLI 测试另行执行通过                                                |
| 真实 OpenCode 2.0.20                                                                         | 1 passed；目录含特殊字符，冷启动发现、两轮文本、只读历史、关闭/恢复及第三轮文本；共六条用户/助手历史 |
| daemon OpenCode 进程测试                                                                     | 2 passed；协议夹具，不冒充真实 CLI                                                                   |
| daemon DSH ACP 进程测试                                                                      | 1 passed                                                                                             |
| 首次输入失败不重发的 CI 回归                                                                 | 1 passed                                                                                             |
| 共享 timeline/stream/cache/question 前端回归                                                 | 313 passed                                                                                           |
| 更新后的 UI CI 选定范围                                                                      | 437 passed，1 skipped；跳过已有的显式 daemon 二进制集成测试                                          |
| DSH provider manifest 与配置 schema                                                          | 14 passed                                                                                            |
| Rustfmt、workspace all-targets Clippy、前端类型检查、变更 TS 格式/lint、文档链接和 diff 检查 | passed                                                                                               |

OpenCode 离线夹具先加入 location 参数校验，旧代码出现 ProviderFailed，修复后通过。
真实 CLI 测试先后复现模型发现失败和回合完成超时，修复后完整通过。
新增离线回归覆盖冷目录最终可用/永久为空、无执行事件时成功/失败恢复、不重发输入、旧 idle 后的新输入、
缺失/逆序时间、未知 outcome 和会话状态不一致。

可执行的主要命令如下（仓库根目录）：

```sh
nix develop --command cargo test -p provider deepseek_harness --locked --offline
nix develop --command cargo test -p provider local::opencode --locked --offline
AIT_TEST_DSH_BIN=/absolute/path/to/dsh nix develop --command cargo test -p provider installed_host_discovers_switches_permissions_and_adopts_legacy_sessions --locked --offline -- --ignored
AIT_TEST_OPENCODE_BIN=/absolute/path/to/opencode-2.0.20 nix develop --command cargo test -p provider installed_opencode_discovers_runs_and_restores_with_local_model --locked --offline -- --ignored
nix develop --command cargo test -p daemon --test process opencode --locked --offline
nix develop --command cargo test -p daemon --test process deepseek_harness --locked --offline
nix develop --command cargo test -p daemon --test process attempted_initial_prompt_is_not_replayed_after_failure --locked --offline
nix develop --command cargo fmt --all --check
nix develop --command cargo clippy --workspace --all-targets --locked --offline -- -D warnings
nix develop --command cargo test --workspace --locked --offline
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-provider-compat/workspace-coverage-summary.json
npm run test --workspace=@ait/mobile -- src/i18n/resources.test.ts src/runtime/rust-daemon maestro/support native-release-version.test.ts src/timeline src/types/stream.test.ts src/runtime/replica-cache src/components/question-form-card-core.test.ts
npm run test --workspace=@ait/protocol -- src/provider-manifest.deepseek-harness.test.ts src/paseo-config-schema.test.ts
npm run typecheck --workspace=@ait/mobile
npm run check:docs
git diff --check
```

同步上游后先重建 UI 依赖；发现本机缺少锁文件已声明的 expo-secure-store 后，按锁文件安装并应用项目 postinstall，
最终类型检查通过，没有更改依赖版本或锁文件中的上游版本。
OpenCode 真实测试使用隔离 XDG 目录和 loopback 确定性模型；不使用用户账号或外部推理。
测试包来自 [Homebrew 官方元数据](https://formulae.brew.sh/api/formula/opencode.json)，下载后验证其 SHA-256。

## Test coverage

提交准备阶段已完成全量普通测试和 LLVM 插桩测试，两轮均为 **1819 passed、0 failed、5 ignored**。
全 workspace Clippy（all-targets、`-D warnings`）、构建和格式检查通过。

| 范围             |   覆盖行 / 总行 | 行覆盖率 |
| ---------------- | --------------: | -------: |
| Workspace        | 50,821 / 53,867 |   94.35% |
| Provider crate   | 22,843 / 24,378 |   93.70% |
| DSH native + ACP |   2,309 / 2,513 |   91.88% |
| OpenCode         |   2,725 / 3,119 |   87.37% |

工具为 cargo-llvm-cov 0.8.7，macOS arm64 / Rust 1.98.1，默认 features、默认文件过滤，无额外排除；
doctest 未插桩。五个忽略项为既有 Claude/Codex CLI 或在线推理测试，以及 DSH/OpenCode 的 opt-in CLI 测试；
后两项已按前文命令单独通过，不计入上述覆盖率。本次没有同一上游基线的可比覆盖率，不计算百分比增量。

[可审阅验证制品](dsh-opencode-group-feedback-2026-10-05.json)包含当前基线、来源提交、源码指纹、
测试命令、日志指纹、workspace/相关 crate 和 provider 逐文件行覆盖率。HTML 位于 `target/llvm-cov/html/index.html`。
尚未覆盖的主要行为包括真实子进程启动异常、部分畸形协议/断线及资源上限路径；后续应扩展对应故障注入与平台回归。

本轮未执行完整 Electron GUI、原生移动端、Windows/Linux，未对真实 DSH 模型执行问答/工具回合。
DSH 问答/审批由原生 HTTP/WS 夹具及共享表单回归覆盖；OpenCode 工具允许/拒绝/顺序和取消由离线回归覆盖，
真实 2.0.20 测试覆盖文本回合及恢复。远端 CI 不计入本报告，推送后的状态以 GitHub 检查为准。

## ACP CI 启动失败修复补充（768674fe 之后）

CI [失败记录](https://github.com/KirisameLonnet/ait/actions/runs/37309033572/job/111759668393)
在创建 ACP 会话时返回 `Unavailable`，定位到 `Command::spawn`，尚未开始协议交互。
原代码丢弃了 OS 错误，不能确认原 CI 的具体 errno。
Linux 容器复现了临时脚本的写入句柄被并发 fork 继承时产生 `ETXTBSY` 的机制；
关闭所有写入句柄后同一脚本连续启动 100 次通过。这是风险验证，不是原 CI errno 的证明。

测试改为执行仓库内带可执行权限的固定 ACP 脚本，避免运行期间创建、写入可执行文件。
每个测试的 cwd 和日志仍独立。新增 16 路并发会话/日志隔离回归，以及程序不存在的错误回归。
生产启动失败新增 tracing 诊断，仅含错误类型和 OS 错误码，不记录路径、参数或环境。
未增加超时、重试或忽略失败测试。

### Test coverage

验证版本为 `768674fe` 合入 upstream `5df233ac`，加随附制品中的源码指纹与 fixture mode。
`nix develop --command cargo test -p provider local::deepseek_harness --lib`：32 passed、1 ignored。
`nix develop --command cargo test --workspace`：1824 passed、0 failed、5 ignored。
`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和
`cargo build --workspace` 均在 Nix dev shell 通过。

`nix develop --command cargo llvm-cov --workspace --html -- --test-threads=1`：
1824 passed、0 failed、5 ignored。

| 范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| workspace | 50830 / 53874 | 94.35% |
| provider | 22847 / 24383 | 93.70% |
| dsh | 2312 / 2518 | 91.82% |

相较 768674fe 同平台同范围测量的行覆盖率变化： workspace +0.0044 个百分点； provider -0.0028 个百分点； dsh -0.0633 个百分点；

macOS arm64 / Rust 1.98.1，默认 features 与文件过滤，无额外排除，doctest 未插桩。
5 个忽略项为既有真实 CLI/在线推理 opt-in 测试，本次未单独运行。
基线为上节 768674fe 的同平台默认 features 测量；系统级故障路径存在运行差异，不将覆盖率差值单独解释为修复效果。
[可审阅覆盖率制品](dsh-acp-ci-2026-10-05.json)包含范围、逐文件数据、源码和日志指纹；
完整 HTML 位于 `target/llvm-cov/html/index.html`。
尚未覆盖所有 OS 启动故障、真实服务断线路径；本地全量测试不代表 Linux/Windows 全量验证。
远端 Linux CI 结果以本补充对应提交的 GitHub 检查为准。
