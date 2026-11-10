# OpenCode 独立 server 迁移

> 历史验证记录：下述路径、命令和指标对应所注明的源码提交；当前目录与上游整合结果见 [PR 验证](opencode-upstream-pr.md)。

日期：2026-09-30。基线：完整 `origin/main` 的 `d5ef2e5`（0.0.11），
与原分支 `5cbab4f` 合并后的待提交树。

## 实现

- 移除 main 已退役的 daemon/worker/Desktop 与专用 crate，保留 flake / direnv。
- OpenCode 注册到新 server，与 Codex/Claude 复用 `AgentClient` / `AgentSession`。
- 迁移原 HTTP/SSE、v1/v2 协议、完整历史、执行日志核对、预算及审批逻辑。
- 新增非阻塞 observer、多轮会话、稳定 display key、客户端消息身份和保存/恢复 handle。
- 历史与 archived resume 只读；原生审批 once，取消经 native acknowledgement，关闭回收进程组。
- 取消确认后核对完整历史并刷新下一轮基线，支持继续输入；无法证明排空时停止 writer。

参考路径及架构决策见 [当前 ADR-074](../../../decisions/providers/adr-074-opencode-native-provider.md)。
旧 Harness 报告与覆盖率制品从 Git 历史查阅，不再保留于当前文档树。

## 使用与限制

本机安装并登录 OpenCode；server 自动发现 `opencode`，或设置 `AIT_SERVER_OPENCODE_BIN`。
支持 OpenCode 1.x 和 2.0.10+，模型 ID 为 `provider/model`，当前模式为 `build`。
原生工具具有完整文件系统访问，命令和编辑需要审批。单次授权不转换为长期规则。

模型发现、文本连续对话、流式文本、shell/edit 审批、取消、重启恢复和只读历史已接通。
plan/custom agents、steer、图片/附件、结构化输出、表单、rewind、外部会话列举/导入、
动态 MCP 和 Provider 环境覆盖尚不支持。恢复固定原有配置；不支持的请求先拒绝。
会话历史上限八 MiB，SSE 单帧 256 KiB；观察预算无法撤销已经执行的工具。

机器未安装真实 OpenCode。测试使用基于 Paseo 原生协议的离线 HTTP 夹具，
不涉及真实登录、模型服务、模型费用或 OpenCode 的第三方插件；这些仍需真实安装验证。
Claude Code 使用 main 已有生产 adapter。

## 验证

| 检查                                                   | 结果                                                               |
| ------------------------------------------------------ | ------------------------------------------------------------------ |
| Rust workspace 构建                                    | 通过                                                               |
| Rustfmt / workspace 全 targets Clippy（`-D warnings`） | 通过                                                               |
| 完整 workspace 测试（LLVM runner）                     | 1507 passed、3 既有在线测试 ignored、0 failed                      |
| OpenCode 协议与新会话 port                             | 22 passed，包含 v1/v2、SSE 最终 key、审批拒绝、取消后继续输入      |
| 实际 server WebSocket 进程测试                         | 通过：模型发现、连续输入、重启恢复、原生提交次数和 helper PID 回收 |
| `git diff --check origin/main`                         | 通过；同步范围包含 main 原有的 Swift/patch 空白，不额外改写        |

检查中发现并修正创建入口的固定 Codex/Claude 白名单、客户端消息身份字段兼容、
旧诊断测试的 Provider 数量断言，以及取消后的历史核对和续执行。
本次没有重跑前端构建与浏览器测试；新前端已有 OpenCode Provider manifest，服务端接入由真实 WebSocket 测试验证。

## Test coverage

| 测量范围（LLVM summary） | 行覆盖率 | 已覆盖 / 总行数 |
| ------------------------ | -------: | --------------: |
| Rust workspace           |   91.47% |   43982 / 48082 |
| server-provider          |   92.11% |   19675 / 21360 |
| OpenCode 生产插件        |   84.28% |     2316 / 2748 |

测量版本为上述两个基线合并后的代码，精确源码 SHA-256、逐文件指标、LCOV 未覆盖行、
命令及日志指纹见 [共享覆盖率制品](opencode-server-coverage.json)。测量后只更新报告和该 JSON。
平台为 macOS arm64，flake 的 rustc 1.98.1 / cargo-llvm-cov 0.8.7，默认 features。
使用 cargo-llvm-cov 默认测试源码过滤，无自定义文件排除；默认未插桩 doctest。
3 项既有在线 Provider 测试保持 ignored，测试通过数量与行覆盖率分别统计。

同口径历史 [GitLab 制品](../workspace/gitlab-forge-coverage.json) 的 workspace 为 91.89%（41648 / 45322），
本次下降 0.42 个百分点；server-provider 从 93.23%（17346 / 18606）下降 1.12 个百分点。
没有重测精确合并父提交，差值不单独归因于本次迁移。旧 daemon/worker 的覆盖率不可作为当前基线。

```bash
nix develop --command cargo fmt --all --check
nix develop --command cargo clippy --workspace --all-targets --locked --offline -- -D warnings
nix develop --command cargo build --workspace --locked --offline
nix develop --command cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
nix develop --command cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-opencode-workspace-summary.json
nix develop --command cargo llvm-cov report --lcov --output-path /private/tmp/ait-opencode-workspace.lcov
```

HTML 位于 `target/llvm-cov/html/index.html`；共享审阅使用上方 JSON，原始 profiles、HTML 与运行数据不提交。
尚未覆盖所有启动超时、强制关闭、异常 HTTP/SSE 和恢复错误路径；这些分支仅部分由离线夹具验证。
真实 OpenCode 发行版、登录/模型服务、第三方插件以及 Linux/Windows 运行仍需后续验证。
