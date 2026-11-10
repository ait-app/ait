# DSH DeepSeek 余额查询修复

## 结论与复现

本机 DSH `0.2.0-rc.2` 配置的是 DeepSeek API Key。原生 `account/getState`
返回 `signed-out`、`account/getBalance` 返回 `null`，只表示没有 DSH 账号登录凭据，
不表示 DeepSeek API Key 无效。原生 `credentials/describe` 确认所用引用来自 `file`；
使用该引用对应的本地 Key 调用 DeepSeek 官方余额 API 成功。

修复前 `DeepSeekHarnessClient::usage` 与会话 `account_usage` 都未实现，继承端口的
`Unavailable`。Manager 将它投影为通用“请去 CLI 登录”提示；并没有真正查询任何余额。
可重复执行的隔离 daemon RPC 测试返回 `status: unavailable`、零窗口，退出码为 1。
新增原生 Host 回归测试先在旧实现上失败，再在修复后通过。

修复后，同一隔离 daemon 测试使用本机 DSH 的现有配置，返回：

```json
{
  "provider": "deepseek-harness",
  "status": "available",
  "windows": 0,
  "balances": 3,
  "units": ["cny"],
  "schemaValid": true,
  "hasRpcError": false
}
```

真实余额、用户配置和凭据没有写入本报告或测试 fixture。测试金额均为合成数据。
测试未发送模型请求，未创建原生对话，也未替换用户正在运行的 Ait desktop daemon。

## 实现

- API Key 路径先读取原生配置中的内置 DeepSeek 插件身份，通过 Host 的安全设置与凭据描述
  选择引用、端点和来源，再读取该来源。支持环境覆盖、版本 1 原生凭据 YAML、项目与 home
  `.env`、自定义引用及静态存储路径。不会通过遍历其他供应商的凭据来猜测 Key。
- 会话保存启动环境快照；查询使用当前 live Host 和该快照，不混入主机账号。
  `.env` 保持启动时语义，DSH 管理的凭据文件仍可随原生更新重新读取。
- 仅请求固定的 DeepSeek 官方 HTTPS 余额接口，不跟随重定向。API Key 只在进程内使用，
  不进入报告、日志、Ait 持久化或命令参数。查询有时限和响应体大小上限。
- 无 API Key 时支持原生 DeepSeek 账号余额；两者存在时优先 API Key，不合并身份。
  未登录、旧 Host 缺少接口、显式 ACP、未知来源或自定义端点有明确不可用说明。
  HTTP 401/403 提示检查 DSH 模型设置中的 Key；超时、服务端错误和损坏响应保持错误状态。
- 投影官方 `total_balance`、`topped_up_balance`、`granted_balance`，保留零值和欠费值。
  总额不由已舍入分项重算；不虚构用量比例、预算上限、重置日期或累计消费。
- 共享协议新增 `cny`，当前与旧版用量组件均支持人民币。Chromium 卡片测试验证中文余额、
  无虚构百分比且无误导的登录错误。daemon 与客户端需使用更新后的构建。
- 沿用现有账号用量 port、Manager 缓存与会话隔离边界，没有新增账号管理或插件执行边界。

## 原生契约依据

[DeepSeek 官方余额 API](https://api-docs.deepseek.com/zh-cn/api/get-user-balance/)定义币种、总余额、
充值余额、赠金及 API 余额可用状态，不提供累计消费或每月套餐窗口。
DSH 契约以本机安装的官方 `@deepseek-ai/dsh` `0.2.0-rc.2` 为准，检查了
`dsh-api-account-controller`、`dsh-api-settings-controller`、`dsh-credentials-local`、
`dsh-llm-deepseek-api-key` 和 `dsh-app-boot`。没有安装第三方插件或改变 DSH 登录配置。

## 验证结果

执行基于 `a5a0f781` 的工作区补丁，在 macOS 上运行。仅测试改动与直接相关行为，未运行完整 Rust workspace 测试。

| 命令 / 验证                                                                                                                                                      | 结果                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `cargo test --locked --offline -p provider --lib local::deepseek_harness`                                                                                        | 71 通过，6 个安装/真实会话测试默认忽略 |
| `AIT_TEST_DSH_BIN=/opt/homebrew/bin/dsh cargo test --locked --offline -p provider --lib installed_deepseek_api_key_balance_without_model_execution -- --ignored` | 1 通过，本机现有 API Key，只读余额     |
| `cargo test --locked --offline -p provider --lib service::agent_manager::usage`                                                                                  | 2 通过                                 |
| `cargo test --locked --offline -p provider --lib service::agent_execution::tests::usage`                                                                         | 1 通过                                 |
| `npm run test --workspace=@ait/mobile -- --project unit src/usage/format.test.ts src/usage/native-report.test.ts src/provider-usage/format.test.ts`              | 7 通过                                 |
| `npm run test --workspace=@ait/protocol -- src/usage-reports.test.ts`                                                                                            | 3 通过                                 |
| `npm run test:browser --workspace=@ait/mobile -- src/usage/card.browser.test.tsx`                                                                                | Chromium 4 通过                        |
| 临时 daemon 的 `provider.usage.list.request`                                                                                                                     | 真实余额 3 项，CNY，共享协议校验通过   |
| `cargo clippy --locked --offline -p provider --all-targets -- -D warnings`                                                                                       | 通过                                   |
| `cargo build --locked --offline --workspace`                                                                                                                     | 通过，无警告                           |
| `cargo fmt --all --check`、改动 TS/TSX 的 oxfmt/oxlint、移动端 typecheck                                                                                         | 通过                                   |
| `npm run build:protocol`、`npm run check:docs`、`git diff --check`                                                                                               | 通过                                   |

失败回归包括：HTTP 401/403/500、重定向、超时、超大响应、损坏 JSON、RPC 关联错误、
遗漏返回值、原生拒绝、未知币种与重复币种、坏金额与损坏 YAML、未知凭据来源、存储中缺失 Key、
静态自定义路径、可执行配置标签、环境与文件来源差异、缓存刷新和停止会话不回退主机。

原生账号登录的成功余额由模拟 Host 覆盖，本机只对实际使用的 API Key 路径完成在线实测。
Linux、Windows 与移动原生运行时未实机验证。测试中显示余额不代表测试消耗了模型额度。

## Test coverage

**not measured**：用户未要求覆盖率测量，本次只执行上述相关测试；通过测试数量不是覆盖率百分比。
本次没有生成可分享的覆盖率产物或可比较基线。若后续要求测量，应使用 `cargo llvm-cov`，
仅选择 DSH Provider 及账号用量相关测试；未经另行要求不扩大到整个 workspace。
