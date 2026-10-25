# OMP ACP 与后台结算实现验证（2026-10-25）

## 改动与边界

Ait 在原 `omp` Provider 下新增可选 ACP，默认保持 RPC。新建草稿的 Protocol 控件
选择协议，恢复 metadata 保存原协议，既有会话不能切换。ACP 复用共享 stdio 传输、
流投影、权限及 form 问答，支持 native new/resume、模型/推理/default-plan 配置、
streaming、steering、取消与原生历史结算。

原生 OMP `18.12.6` / `45c35e40c46166dc726ccc52f5d27f40f2449f99` 同时增加
显式协商的 `_session/settled` 扩展。补丁及所有新增/修改的原生测试保留在
[third-party/oh-my-pi](../../../third-party/oh-my-pi/README.md)，没有替换系统 `/usr/bin/omp`。
Ait 本身保持 Rust 实现；没有新增 domain、ports 或共享客户端 wire 类型。

## 可观察行为

- `agent_end` 不提前结束有后台任务的原生 ACP prompt；等待 owner jobs/deliveries，
  仍在同一订阅中向宿主转发续跑。最终 event 而非首次 yield 决定错误或成功。
- 本轮成功响应与相同 session/turn 的 settled 都到达才结束；忽略上一轮的信号，
  不以延时判断完成，不因传输不确定自动重发输入。
- 已完成/旧回合工具的迟到进度不会重新打开 Timeline 项。当前工具继续正常更新。
- steering 等控制响应之前到达的审批 callback 被保留，随后仍可回答；不会被初始化
  阶段的默认拒绝逻辑吞掉。问答支持含逗号的多选项并校验唯一性。
- 原生 provider 错误通过 `_meta.error` 进入脱敏失败路径；带 `providerError` 标记的
  原生错误文本不会作为普通答案写入 Timeline。失败原因在关闭后仍可读取。
- 原生历史以只读方式在 host 终态前校正，保留用户回执、协议和恢复身份；
  空会话可以重建且不制造输入，已提交历史缺失时明确失败。
- 本机未修改版本仍可使用 RPC；ACP 要求带上述补丁的原生实现，缺少协商能力时在
  发送输入前拒绝，避免丢失后台续跑。未启用扩展的普通 ACP 客户端行为保持兼容。

## 验证命令

Ait 使用仓库要求的 Rust 1.98.1，针对 OMP、共享 ACP 投影、受提取影响的
OpenCode/DSH 和诊断模块执行测试。未运行整个 Rust workspace 的测试。

```sh
cargo test --locked -p provider local::omp --lib
cargo test --locked -p provider local::acp_stream --lib
cargo test --locked -p provider local::opencode --lib
cargo test --locked -p provider local::deepseek_harness --lib
cargo test --locked -p provider local::diagnostics --lib
cargo fmt --all --check
cargo clippy --locked -p provider --all-targets -- -D warnings
npm run check:docs
```

原生 OMP 在隔离源码目录中安装依赖，并使用同版本官方 npm native addon。
以下测试采用 mock 模型和隔离历史，没有调用付费模型：

```sh
bun test packages/coding-agent/test/acp-agent.test.ts packages/coding-agent/test/acp-initialize-conformance.test.ts packages/coding-agent/test/acp-mcp-isolation.test.ts packages/coding-agent/test/agent-session-async-delivery.test.ts
bun run --cwd packages/coding-agent check:types
bun x --no-install oxfmt --check packages/coding-agent/src/main.ts packages/coding-agent/src/modes/acp/acp-agent.ts packages/coding-agent/src/session/agent-session.ts packages/coding-agent/test/acp-agent.test.ts packages/coding-agent/test/acp-initialize-conformance.test.ts packages/coding-agent/test/acp-mcp-isolation.test.ts packages/coding-agent/test/agent-session-async-delivery.test.ts
bun x --no-install oxlint packages/coding-agent/src/main.ts packages/coding-agent/src/modes/acp/acp-agent.ts packages/coding-agent/src/session/agent-session.ts packages/coding-agent/test/acp-agent.test.ts packages/coding-agent/test/acp-initialize-conformance.test.ts packages/coding-agent/test/acp-mcp-isolation.test.ts packages/coding-agent/test/agent-session-async-delivery.test.ts
```

原生测试最终结果为 131 passed、0 failed；类型检查通过。Ait 最终结果：OMP 41 passed / 3 ignored；共享 ACP stream 5 passed；
OpenCode 99 passed / 28 ignored；DSH 87 passed / 7 ignored；诊断 20 passed。
共 252 passed、38 ignored、0 failed。跳过项要求指定真实 CLI、外部环境或付费模型；
未运行这些未显式启用的集成测试。format、provider clippy 与文档检查均通过。
修改基线为 Ait `7c824d0a`（工作分支 `fix/omp-failure-propagation`）以及上述 OMP revision；
测试针对未提交改动执行。没有发布、推送或修改运行中 daemon。

真实打补丁 CLI 还在隔离配置下完成了 initialize、session/new、configOptions、
本地 `/model` 命令、与 `wire-smoke` turn 关联的 `_session/settled` 和 session/close，
没有调用模型；原生 `--no-session` 不产生会话 JSONL。
并行测试发现临时写入后立即执行的 Python 脚本偶发启动失败，夹具改为执行仓库中
不可变的脚本，保留每个测试独立 cwd 和状态目录，没有降低测试并行度。

## Test coverage

未测量：用户未要求覆盖率；通过项数不是行覆盖率。若后续需要，使用
`cargo llvm-cov` 对上述实际受影响模块做定向测量，并附报告和明确排除项。
本次不推测覆盖率百分比，也不以报告要求扩大到完整 workspace。
