# DeepSeek Harness 原生 Host 验证

后续 `aa438db5` 桌面复测及新增展示修复见[展示恢复报告](deepseek-harness-display-recovery.md)。
以下数字保留为本阶段的历史测量。

日期：2026-10-04。对应 PR #169 的 `feat/dsh-native-host`。
本轮在 `32d94dc2f092baa53f0bcf80c16faf340725cfbf` 上修复验收缺陷，并合入上游
`2b55e3908b3421678271a9136d35ae20b4941fde`（含 PR #170 与 0.0.16 发布整合 #167）。覆盖率附件记录两个父提交和实际源码 SHA-256。

## 交付范围

默认 DSH adapter 使用原生交互 Web Host，补齐权限模式和 question。
Rust 原生 HTTP/WebSocket 协议实现留在 DSH provider 下；Codex 和 OpenCode adapter 不变。
共享 question 表单仅增加可选数组答案，原有字符串答案路径保持兼容。
[ADR-082](../../decisions/providers/adr-082-deepseek-harness-native-host.md)记录边界与兼容入口。

本轮修复桌面测试在 `32d94dc2` 确认的两项缺陷：

- **P1 用户消息消失**：处理原生 `user/message`，保留 message ID，并用原生 request ID 关联客户端消息。
  实时显示和只读历史恢复共用投影；固定 cursor 分页读取已登记会话的完整原生记录，
  通过既有原子 reconcile 修复旧缓存遗漏。稳定键避免重复，历史有缺口时不会覆盖已有记录。
  恢复不会 create/resume writer，也不会重新提交 prompt。
- **P2 空工具参数中断整轮**：遵循官方 agent loop 的解析规则，精确空字符串转 `{}`，合法 JSON 保留值，
  非法 JSON 原样保留。后续工具校验失败显示为工具结果，助手仍可继续。

## 测试执行

- DSH 定向回归：**29 passed、0 failed、1 ignored**，覆盖 native Host 与既有 ACP。
  新回归覆盖空参数、非法 JSON、纯空白、null、数组、对象；六轮用户/工具/助手顺序；
  工具 ID 跨回合复用、多页旧历史恢复、重复 reconcile 不换 epoch、SQLite 关闭重开、历史缺口拒绝、
  未进入活动回合的用户消息、用户图片/附件，以及恢复时不重发输入。
- 官方 DSH `0.1.5-rc.2` 单独烟雾测试：**1 passed**。隔离 `DSH_HOME`，真实进程和回环网络，
  模型发现、只读权限、关闭/恢复、旧 ACP handle 接续，以及新增只读历史接口均通过；不请求模型。
- `cargo fmt --all --check`、全 workspace clippy（all-targets、`-D warnings`）、build 和普通测试通过。
  普通全仓库测试 **1786 passed、0 failed、4 ignored**；覆盖率执行同样通过。
  合入上游依赖集中管理后首次离线运行缺少 onig 缓存，联网获取锁定依赖后完成检查。
- 文档链接检查通过。本轮没有更改前端；原提交的 question form **5 passed**、协议包 **763 passed**、
  SDK/UI 依赖构建、全量移动/Web 类型检查及变更文件格式/lint 结果仍为 `32d94dc2` 的历史证据，未冒充本轮重测。

模拟 Host 集成使用真实本地 HTTP/WS 服务，但不代表真实模型推理或桌面验收。
用户提供的外部报告已验证 `32d94dc2` 的真实 DSH 审批、问答、取消及重启继续；
同一报告发现上述 P1/P2，因此旧提交未验收通过，修复后的桌面结果仍待复测。

## Test coverage

| Rust 范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| Workspace | 50,385 / 53,391 | 94.37% |
| Provider crate | 22,658 / 24,185 | 93.69% |
| DSH（native + ACP） | 2,302 / 2,506 | 91.86% |
| DSH native Host | 1,293 / 1,432 | 90.29% |

平台 `aarch64-apple-darwin`，Rust 1.98.1、cargo-llvm-cov 0.8.7，默认 features 和默认文件过滤，
无额外排除，不包含 doctest 行覆盖率。4 个忽略测试中真实 DSH 烟雾已单独执行，其余为既有 CLI/在线推理测试。
相对 `32d94dc2` 的 native Host 同范围基线 **1,055 / 1,186（88.95%）**，本轮变化 **+1.34 个百分点**。
workspace/provider 同时含上游 PR #170 / #167 的改动，不把其变化全部归因于这两项修复。
前端覆盖率未测量，测试通过数量不代表行覆盖率。

在 Nix dev shell 中设置
`CARGO_TARGET_DIR=/Users/lonnetkirisame/Documents/Developer/ait/target` 后执行：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-dsh-fixes-coverage.json
```

[可审阅 JSON 附件](deepseek-harness-native-host-coverage.json)包含逐文件指标、源码 SHA-256、
基线、工具/平台范围和测试日志摘要。HTML 位于共享 target 的 `llvm-cov/html/index.html`。
烟雾命令为设置 `AIT_TEST_DSH_BIN=/etc/profiles/per-user/lonnetkirisame/bin/dsh` 后运行
`cargo test -p provider installed_host_discovers_switches_permissions_and_adopts_legacy_sessions --locked --offline -- --ignored`。

主要缺口为部分启动/异常退出、畸形 RPC、历史读取大小上限等失败分支。
本轮未验证 Electron UI、真实模型推理、Debian amd64、Windows 或原生移动端。
现有 daemon 进程级 DSH 测试显式使用 ACP fixture；native Host 由 provider 网络集成和真实 CLI 烟雾覆盖。

## 桌面复测交接

使用本分支修复后的固定提交，不使用 `32d94dc2` 或浮动 main。启动前确认没有设置
`AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp`，升级后重启 Ait daemon。

1. 打开旧版已有用户消息缺失的 DSH 会话，不发送新问题，确认原生记录中的用户气泡自动恢复。
2. 新建会话，多轮文本、图片/附件、question 与工具执行；刷新、切换工作区、重启后用户/工具/助手顺序稳定，无丢失或重复。
3. 让本地测试模型发出工具参数 `""`、非法 JSON、纯空白与合法参数。空值应按 `{}` 进入原生工具校验；
   参数错误可显示工具失败，但适配器不应报整轮 Provider execution failed，助手应能继续。
4. 回归单选/多选/自定义问题、权限允许/拒绝/只读、重复点击、取消后继续和旧 ACP 会话接续。
5. 回归 Codex、Claude、OpenCode 的原有 question 字符串回答路径。

报告请注明 commit、DSH 版本、系统、模型来源，附失败步骤、截图和脱敏日志。
