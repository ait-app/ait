# Codex computer use 请求处理与 Auto-review 批准

日期：2026-10-10。平台：macOS aarch64。基准提交：
`4fecd83a97fd7fedb12e79a2ad15607f1978e3ed`，测量包含本次变更。
测量源码摘要：`debbac2c7575c03d74be30d1bb4636fd9f13d8069378ea70e58c9b93135cefa0`。
本机原生 CLI：`codex-cli 0.160.0`。

## 检查结论与变更

Ait 已将 `cua_repl.js` 的原生 `mcpToolCall` 投影为稳定的工具卡片，保留参数、
结果与原生错误。实时输出和历史使用同一名称与调用身份；截图被转成私有图像引用，
大型截图元数据不会进入时间线。

阻塞式 `item/tool/requestUserInput` 保留问题 ID 与选项，回复转换成原生
`answers[id].answers`。MCP elicitation 的普通 primitive form 转成已有问答 UI，
返回经过类型校验的 `content`；拒绝/取消转换为原生 `decline`/`cancel`。
线程、轮次、请求 ID 与待处理数量均有检查。

这些交互不等同于支持全部 computer use 授权界面：非阻塞原生问题仍显式不支持；
URL、`openai/userVerification` 和不支持的嵌套 MCP form 在 transport 层被拒绝。
本次没有添加这些原生交互的 UI，也没有实际操作浏览器或桌面来验证它们。

Auto-review 原来只设置 `approvalsReviewer=auto_review`，没有配置插件 MCP 工具的
独立审批策略。本次直接在该档位预批准 `cua_repl.js`，不增加独立开关。
配置位于插件所属路径，见 [ADR-122](../../decisions/providers/adr-122-codex-computer-use-approval.md)。
切入和退出 Auto-review 都会重新启动原生进程并恢复同一线程，解决档位变化未重载
线程级工具配置的问题。显式逐工具设置覆盖档位默认值。

## 原生 CLI 验证

使用独立的临时 `CODEX_HOME`，通过 `codex app-server` 的 stdio 接口初始化，
发送下列 `thread/start` 参数，没有调用 `turn/start` 或模型：

```json
{
  "cwd": "<temporary directory>",
  "ephemeral": true,
  "approvalPolicy": "never",
  "sandbox": "read-only",
  "config": {
    "plugins": {
      "unified-computer-use@openai-bundled": {
        "mcp_servers": {
          "cua_repl": {
            "tools": { "js": { "approval_mode": "approve" } }
          }
        }
      }
    }
  }
}
```

`approve` 配置被接受。将该值改为 `invalid-for-audit` 后，原生 CLI 返回配置错误，
明确指出同一路径只接受 `auto`、`prompt`、`writes`、`approve`，确认字段被实际解析。
另执行 `codex app-server generate-ts --experimental --out <temporary directory>`，
核对阻塞问答与 MCP elicitation 请求/回复类型。

原生验证证明配置受支持，不是完整的 computer use 操作验收。
应用/网站访问、身份验证和系统权限可能仍产生独立请求。
官方依据：[插件逐工具配置](https://developers.openai.com/codex/config-reference/)和
[MCP 工具调用批准](https://learn.chatgpt.com/docs/app-server#mcp-tool-call-approvals-apps)。

## 测试与静态检查

- `cargo nextest run --workspace`：2039 项通过，0 项失败，16 项既有 installed
  原生 provider 测试跳过；这些测试需要本机 provider、认证或模型请求。
- `cargo test --workspace --doc`、`cargo build --workspace`：通过，无编译警告。
- `cargo llvm-cov nextest --workspace --html`：2039 项通过，0 项失败，16 项跳过。
- 新增回归覆盖 Auto-review 的精确工具范围、其他档位不注入、显式设置优先、
  非法设置拒绝、同线程进入/退出 Auto-review，以及 `Accept`/`Decline`/`Cancel`
  选项的原生回复往返。已有截图、历史、MCP form、线程与轮次校验测试也通过。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `npm run check:docs`：通过。

## Test coverage

| 测量范围 | 行覆盖率 | 已覆盖 / 总行数 |
| --- | --- | --- |
| Rust workspace | 94.68% | 56552 / 59730 |
| `provider` crate | 94.19% | 27057 / 28726 |

精确命令：

```sh
cargo llvm-cov clean --workspace
cargo llvm-cov nextest --workspace --html
cargo llvm-cov report --json --output-path /tmp/ait-cua-pr-coverage-full.json
```

测量使用上方基准提交加本次源码变更、默认 features、macOS aarch64；没有添加文件排除，
沿用 cargo-llvm-cov 默认源码过滤。16 项既有原生环境测试跳过，doctest 单独检查但未插桩，
没有运行 Linux 或 Windows。源码摘要覆盖排序后的受版本控制的 Cargo 配置、nextest 配置、
`bins/` 和 `crates/` 文件，确认测试期间这些源码没有变化。
没有该基准提交的可比覆盖率测量，不报告覆盖率增减。

可审阅的[覆盖率摘要产物](codex-computer-use-approval-coverage-2026-10-10.json)
包含 workspace、逐 crate、变更生产文件的覆盖行数及测量范围。
本地 HTML 报告位于 `target/llvm-cov/html/index.html`；已审阅新增配置辅助函数及档位重载条件，
新增可执行行均被覆盖。原始 LLVM 数据和 HTML 留在临时目录与忽略的构建目录中。
尚未覆盖真实浏览器/桌面操作及独立访问授权的端到端行为；需有真实原生环境时继续验证。
