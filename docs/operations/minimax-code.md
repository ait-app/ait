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

当前验证版本为 **0.7.1**（2026-10-28 查询 npm 的 latest）。如果 npm 阻止安装脚本，
MiniMax Code 的 SQLite 组件需要显式允许；支持 `--allow-scripts` 的 npm 可执行：

```sh
npm install --global @minimax-ai/code@latest --include=optional --ignore-scripts=false \
  --allow-scripts=@minimax-ai/code,better-sqlite3
```

重新启动包含此实现的 AIT daemon，刷新 Provider 目录，选择 MiniMax Code 创建会话。
模型、思考等级、工作模式和权限选项取自已登录的原生程序。
`Default`/`Plan` 是工作模式，`Permissions` 是原生工具审批策略，两者分别设置。
思考等级取决于当前模型：本次原生程序为 M3.1-Flash thinking 提供等级选择，
切换至 M3 thinking 后不再提供该选项；AIT 不向不支持的模型强行传入思考等级。

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
cargo test --locked -p provider --lib local::minimax
```

真实测试需要已经登录的原生程序，会调用模型并在临时目录执行测试任务：

```sh
AIT_TEST_MINIMAX_CODE_BIN="$(command -v mcode)" \
  cargo test --locked -p provider --lib local::minimax::tests::live -- --ignored --nocapture
```

浏览器真实联调使用独立的临时 daemon 和工作区，不连接正在使用的 AIT 服务：

```sh
AIT_TEST_MINIMAX_CODE_BIN="$(command -v mcode)" \
  npm exec --workspace=@ait/mobile -- playwright test --project=real-provider \
  e2e/browser/minimax-code.real.spec.ts
```

[设计决策](../decisions/providers/adr-197-minimax-code-acp.md) ·
[本次验证](../reports/providers/minimax-code-acp-2026-10-28.md)
