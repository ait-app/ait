# DeepSeek Harness 原生 Host 验证

日期：2026-10-04。基线：上游 `151ce454867d915b5f80823463d9b8eafd0283ae`。
本报告对应 `feat/dsh-native-host` 的源码，最终覆盖率附件记录源码 SHA-256，后续只更新报告。

## 交付范围

默认 DSH adapter 改为原生交互 Web Host，补齐权限模式和 question。
Rust 原生 HTTP/WebSocket 协议实现留在 DSH provider 下；Codex 和 OpenCode adapter 不变。
共享 question 表单仅增加可选数组答案，原有字符串答案路径保持兼容。
[ADR-082](../../decisions/providers/adr-082-deepseek-harness-native-host.md)记录边界与兼容入口。

## 测试执行

- DSH 定向回归：26 passed，0 failed，1 ignored；覆盖新原生 Host 与既有 ACP。
- 安装的官方 DSH `0.1.5-rc.2`：隔离 `DSH_HOME`、真实进程和回环网络，
  模型发现、只读权限、关闭/恢复、旧 ACP handle 接续通过；不请求模型。
- Question form 定向测试：5 passed，含带逗号的多选标签与自由文本，不改变旧 provider 答案格式。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings`、
  `cargo build --workspace --locked --offline` 通过；普通全仓库测试 **1,781 passed、0 failed、4 ignored**。
- SDK/UI 依赖构建、移动/Web 全量类型检查、变更文件 oxfmt/oxlint、文档链接检查通过。
- 协议包测试 **763 passed**。首次与覆盖率编译并发时出现一个 5 秒超时，限制 4 workers 后全量通过。
- 覆盖率首次 4 test threads 运行遇到已有目录订阅等待事件超时；普通测试已通过，串行覆盖率复测全部通过（1,781 passed、0 failed、4 ignored），未改动该测试或放宽断言。

离线集成测试运行真实本地 HTTP/WS 服务，覆盖 cookie 鉴权、RPC 关联、模式与推理选择、
审批先于工具历史到达、允许/拒绝、question 回传、重复/过期拒绝、取消后继续、
工具先于结论、图片落盘、上下文占用、跨 agent 委派、历史序号缺口失败及恢复不重发输入。
测试 peer 是模拟 Host，不代表真实模型推理或桌面验收。

## Test coverage

| Rust 范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| Workspace | 50,110 / 53,107 | 94.36% |
| Provider crate | 22,382 / 23,900 | 93.65% |
| DSH（native + ACP） | 2,051 / 2,247 | 91.28% |
| DSH native Host | 1,055 / 1,186 | 88.95% |

平台 `aarch64-apple-darwin`，Rust 1.98.1、cargo-llvm-cov 0.8.7，默认 features 和
cargo-llvm-cov 默认文件过滤，无额外排除。4 个忽略测试包含真实 DSH 烟雾测试；
它已单独运行通过，其余是既有 CLI/在线推理测试。没有可比的 native Host 基线，故不计算百分比变化。
前端行覆盖率未测量；5 项表单测试和 763 项协议测试是执行结果，不是覆盖率。

在 Nix dev shell 中，设置
`CARGO_TARGET_DIR=/Users/lonnetkirisame/Documents/Developer/ait/target` 后执行：

```sh
cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-dsh-native-coverage.json
```

[可审阅 JSON 附件](deepseek-harness-native-host-coverage.json)包含逐文件指标、源码 SHA-256、
基线提交、工具/平台范围和测试日志摘要。HTML 在共享 target 的 `llvm-cov/html/index.html`。
真实 DSH 烟雾命令为设置 `AIT_TEST_DSH_BIN=/etc/profiles/per-user/lonnetkirisame/bin/dsh` 后运行
`cargo test -p provider installed_host_discovers_switches_permissions_and_adopts_legacy_sessions --locked --offline -- --ignored --nocapture`。

未覆盖行为主要是部分启动/异常退出、畸形 RPC 响应和验证上限分支；
真实模型、生产 daemon 的原生 Host 路径、桌面交互和其他平台由下方复测清单补充。

## 桌面复测交接

使用本分支最终提交，而不是浮动 main。Debian 13 amd64 与本机桌面均需安装并配置 DSH，
启动前确认没有设置 `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp`。

1. 创建 DSH 会话；验证模型/推理选项与三种权限模式，切换后发送新输入，检查 DSH 实际执行限制。
2. 触发工具审批，分别允许、拒绝、取消；确认审批卡包含实际工具输入，重复点击不重复执行。
3. 触发单选、多选、含逗号选项及自由文本 question，提交后原回合继续；拒绝和取消后可再次输入。
4. 多轮、连续输入、工具结论排序；刷新、重启和工作区切换后，无丢失、重复和错序。
5. 老 ACP 会话恢复、图片输入/输出、上下文用量；已有 MCP override 会话使用显式 ACP 或迁移 DSH 配置。
6. 回归 Codex、Claude、OpenCode 的现有 question 表单，确认旧字符串回答路径保持原样。

测试报告请标注 commit、DSH 版本、系统、模型来源，附失败场景的步骤/截图和脱敏日志。
真实模型推理、Electron UI、Debian/Windows 和原生移动端尚未验证，需由桌面测试补充。
现有 daemon 进程级 DSH 回归显式使用 ACP fixture；原生 Host 的网络集成由 provider 测试覆盖。
