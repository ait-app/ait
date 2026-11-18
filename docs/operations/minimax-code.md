# MiniMax Code

AIT 使用 MiniMax Code 官方 ACP 接口，Provider 名称为 **MiniMax Code**，标识为
`minimax-code`。这是 Paseo 使用的接入方式；`mmx` 多媒体/API CLI 是另一个程序。

## 安装与登录

使用 Node.js 22.19–22.x 或 24–26.x，安装当前最新版并完成原生登录：

```sh
npm install --global @minimax-ai/code@latest --include=optional
mcode --version
mcode login
```

当前验证版本为 **0.7.7**（2026-11-18 本机安装版本）。如果 npm 阻止安装脚本，
MiniMax Code 的 SQLite 组件需要显式允许；支持 `--allow-scripts` 的 npm 可执行：

```sh
npm install --global @minimax-ai/code@latest --include=optional --ignore-scripts=false \
  --allow-scripts=@minimax-ai/code,better-sqlite3
```

重新启动包含此实现的 AIT daemon，刷新 Provider 目录，选择 MiniMax Code 创建会话。
模型、思考等级、工作模式和权限选项取自已登录的原生程序。
`Default`/`Plan` 是工作模式，`Permissions` 是原生工具审批策略，两者分别设置。
原生模型目录把每个模型按推理变体分别列出（如 `M3`、`M3 · thinking`，值为
`m:minimax:MiniMax-M3:v:` 与 `m:minimax:MiniMax-M3:v:thinking`），思考等级又是另一个
只对部分变体提供的选项。AIT 把两者拆开：模型选择器每个模型只列一次，思考选择器列出
该模型被原生确认的变体与等级：

| 思考选项 ID       | 原生选择                                   |
| ----------------- | ------------------------------------------ |
| `off`             | 非推理变体（值以 `:v:` 结尾）              |
| `thinking`        | `thinking` 变体，不设置思考等级            |
| `thinking:<等级>` | `thinking` 变体，并把思考等级设为 `<等级>` |

以 `mcode 0.7.7` 为例：`M3.1-Flash` 提供 Default/Low/Medium/High/Xhigh/Max，`M3` 提供
Off/Thinking。未指定思考选项时，AIT 优先保持当前变体，原生拒绝时依次尝试该模型的其他
变体。旧会话保存的完整原生模型值仍按原值应用，运行时信息会报告拆分后的模型与思考选项。
不使用 `:v:` 标记的模型值保持原样，思考选项就是原生思考等级。

模型目录会在临时原生会话中验证每个变体，并分别读取该变体确认的思考等级。
验证不发送提示词、不执行工具，也不修改原生默认模型或权限；整轮验证最多 30 秒。
原生明确拒绝的变体不出现在思考选项中，全部变体被拒绝的模型标为不可选；
传输断开或超时仍作为发现失败处理。

`mcode` 0.7.1 至 0.7.7 列出的非 thinking 版 `M3.1-Flash` 在
`session/set_config_option` 返回 `Invalid model reasoning`。AIT 根据实际确认结果
去掉该项（`M3.1-Flash` 不提供 Off），不硬编码模型黑名单，也不擅自替换用户所选模型。

daemon 默认从 PATH 查找 `mcode`。自定义安装位置使用：

```sh
export AIT_SERVER_MINIMAX_CODE_BIN=/absolute/path/to/mcode
```

该变量只填写可执行文件路径；adapter 会追加 `acp`，不经过 shell。
宿主继承原生环境；不要把凭据写入仓库或作为启动参数提交。

## 已接入能力

- 原生模型与推理等级、Default/Plan 模式和独立权限选择。
- 文本/图片输入、流式回答、推理、工具进度、审批和原生 token 用量。
- 停止原生回合及受管进程清理；原生错误脱敏后保留在 AIT 中。
- AIT 会话关闭/重启后恢复 MiniMax Code 原生上下文和 AIT 展示历史。
- 在新会话启动时传入 MCP servers 与附加 system prompt；这些启动配置不能在同一
  原生进程内切换。

当前不提供 MiniMax Code 外部会话导入、回退、实时语音或辅助摘要。
本次也没有启用任意自定义 ACP provider 的动态注册。

## 验证入口

离线测试不访问模型服务：

```sh
cargo test --locked -p provider-minimax
```

真实测试需要已经登录的原生程序，会调用模型并在临时目录执行测试任务：

```sh
AIT_TEST_MINIMAX_CODE_BIN="$(command -v mcode)" \
  cargo test --locked -p provider-minimax --lib tests::live -- --ignored --nocapture
```

浏览器真实联调使用独立的临时 daemon 和工作区，不连接正在使用的 AIT 服务：

```sh
AIT_TEST_MINIMAX_CODE_BIN="$(command -v mcode)" \
  npm exec --workspace=@ait/mobile -- playwright test --project=real-provider \
  e2e/browser/minimax-code.real.spec.ts
```

[设计决策](../decisions/providers/adr-197-minimax-code-acp.md) ·
[本次验证](../archive/reports/providers/minimax-code-acp-2026-10-28.md)
