# OMP 官方兼容、ACP 桥接与通用插件验证（2026-10-25）

## 实现与范围

本报告对应所在提交的最终桥接实现，基线为 Ait `7c824d0a`，Linux x86_64、Rust 1.98.1。
OMP Provider 的新建和恢复固定经过 Ait Rust ACP 引擎，底层由一个官方 OMP RPC 实例持有
对话、控制、MCP、插件、子 Agent、后台任务和原生历史。额外的能力探测为无会话临时实例。

相同引擎提供 `daemon omp-acp` JSONL stdio，明确标识 Ait 桥接层和 RPC 后端。
这不是直连 OMP 自带 ACP，不再附带或要求安装 OMP 源码补丁。
旧 `transport: rpc/acp` 设置仅作输入兼容，新句柄同时记录 ACP 入口与 RPC 原生后端。

决策见 [ADR-190](../../decisions/providers/adr-190-omp-acp-settlement.md)，
[架构与兼容表](../../architecture/omp-acp-bridge.md)记录接口及真实限制。

## 官方接口核对

实测使用未修改的 `/usr/bin/omp` **18.12.6**，对应
[官方 revision](https://github.com/can1357/oh-my-pi/tree/45c35e40c46166dc726ccc52f5d27f40f2449f99)。
其 [ACP 实现](https://github.com/can1357/oh-my-pi/blob/45c35e40c46166dc726ccc52f5d27f40f2449f99/packages/coding-agent/src/modes/acp/acp-agent.ts)
没有此前定制方案的 native-feature/session-settled 版本标志；config options 为 mode/model/thinking，
boolean autoRetry 请求被拒绝。官方 ACP 和 RPC 运行实例独立，不能靠第二个 RPC 进程
观察第一个 ACP 进程的后台任务或子 Agent。

因此将转换放到 Ait，直接使用官方已有 RPC 契约。此前定制 OMP 的测试结果不作为本报告证据。
标准 ACP 请求、原生额外控制和事件由同一个桥接引擎处理，不对插件名称写分支。

## 回归与实际插件

| 范围          | 证据                                                                                                              |
| ------------- | ----------------------------------------------------------------------------------------------------------------- |
| 既有 OMP 行为 | 新建/恢复、当前分支、用户回执、模型与服务档位、审批/问答、steering/取消、图片、用量、子 Agent、大输出与错误持久化 |
| 后台工作      | 原生关联 prompt_result + session_settled，前后台并行与迟到工具进度，不在中间 agent_end 结束或关闭连接             |
| 标准 ACP      | JSONL initialize/new/load/prompt/config/close，历史回放、断帧读写、请求关联、未知方法及错误响应                   |
| 通用插件      | 两个不同名称的插件命令共用提问/回答/自定义消息路径；命令可先交互后确认，避免等待确认造成死锁                      |
| 原生扩展      | 控制也异步关联；原生 UI、host tool callback 和未知事件字段通过协商通道保留，回调校验 owner/id/type                |
| 错误透传      | 原生原因与 HTTP 状态进入 ACP 失败响应；错误字段有界脱敏，关闭后原因仍保留，不把工具超时误判成 Provider 失败       |
| 旧配置        | 旧 RPC 选择与没有 transport metadata 的句柄仍进入桥接层，保留 native session ID                                   |

用户已明确授权调用其 OMP Provider 中的实际模型。实测继承本机 OMP 原生模型、认证、
插件和权限配置，不复制登录凭据、不改写用户配置；测试工具只操作临时工作目录。

- `tai/deepseek-v4.1-flash`：真实流式推理及 token usage；写入测试文件、原生审批、
  结构化 ask 选项；原生 JSONL 历史和会话列表；关闭后恢复同一个原生会话，模型正确记起首轮随机标识。
- 同一实际模型配合官方 `--extension` 临时加载通用 ExtensionAPI 测试扩展：
  notify-only 输出、两个不同命令名称的 select/custom messages、动态注册工具、历史和恢复。
  工具可能经 OMP 自身 deferred dispatch 执行；同时核对原生成功 toolResult，
  不假设外层工具名称必须等于插件工具名称。生产适配器没有该测试插件的名称分支。
- `futureppo/deepseek-v4.1-flash`：上游返回真实 HTTP 503（模型无可用渠道）；
  Ait 保留原生解释和更换模型建议。该次不计为成功推理，也不重发用户输入。

真实测试发现并修正：notify/command_output 被吞掉，以及官方 RPC prompt 确认的 `data`
为空时被误认为原生历史已落盘。独立 stdio 测试还发现模型/推理/开关更新会重启进程，
现改用官方 RPC 原地更新，并断言正式进程只有一个，保留插件状态和原生会话身份。新增回归验证只有通知的命令仍显示输出，未落盘会话可按空会话重开。

另保留可离线重复的本地 SSE 模型测试作为回归用例；它不作为本次真实模型兼容的主要证据。
测试扩展只由测试启动参数加载，不安装到用户 OMP，不是使用 Ait 的前提。
仅插件本地输出时 OMP 可能延迟落盘；任意 TUI 组件工厂等官方 RPC 未导出的能力不宣称已支持。

## 检查命令与结果

使用仓库固定的 Rust 1.98.1，仅运行修改及直接相关代码：

```sh
cargo test --locked -p provider --lib local::omp
cargo test --locked -p provider --lib local::diagnostics
AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider --lib local::omp::tests::installed -- --ignored
AIT_TEST_OMP_BIN=/tmp/ait-omp-official-extension-test AIT_TEST_OMP_REAL_MODEL='["tai","deepseek-v4.1-flash"]' cargo test --locked -p provider --lib local::omp::tests::real -- --ignored --nocapture
cargo test --locked -p daemon --bin daemon config::tests::omp_acp_arguments_preserve_executable_paths_without_daemon_configuration
cargo clippy --locked -p provider -p daemon --all-targets -- -D warnings
cargo build --locked -p daemon
cargo fmt --all --check
npm run check:docs
```

`/tmp/ait-omp-official-extension-test` 为测试 shell 入口，仅执行
`/usr/bin/omp --extension <仓库测试 fixtures/generic-extension.ts> "$@"`，没有修改 OMP。
实际模型路由由 `AIT_TEST_OMP_REAL_MODEL` 指定，认证仍由原生配置读取。

结果：OMP 定向回归 **53 passed / 6 ignored**；共享诊断 **20 passed**；
未修改官方 CLI 检查 **4 passed**；真实模型/插件 **2 passed**；daemon CLI 参数 **1 passed**。
独立 `daemon omp-acp` 使用实际模型的标准 JSONL 联调通过：初始化、设置、原生控制、流式推理、
load 历史与恢复上下文、EOF 正常退出；不初始化 HTTP daemon、Host 登录或本地 API token。
模式变更前的 ACP ID 在空原生会话重开后仍可用于重新连接和恢复。
格式、严格 Clippy、daemon 构建和文档链接检查通过。不沿用已撤回的定制 OMP 结果。
未运行完整 Rust workspace 测试或真实远程 MCP。

## Test coverage

**Not measured**：用户未要求行覆盖率，本次没有生成覆盖率工件。
通过项数不代表覆盖率百分比。若后续要求，使用 `cargo llvm-cov` 限定 OMP、共享诊断和
新增 CLI 代码，并提供可评审报告；不自动扩大为完整 workspace 测试。
