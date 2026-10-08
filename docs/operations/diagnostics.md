# 故障诊断与证据导出

在设置的「应用诊断」中运行诊断，点击导出按钮保存 `.txt` 文件。桌面和浏览器下载文件，
Android / iOS 打开系统分享面板。将文件作为附件交给开发者，无须把长文本粘贴进聊天。
报告仍可以复制；离线主机会标明未连接，桌面本地日志仍可收集。

## 自动证据

运行中的 daemon 会记录 WARN/ERROR、RPC 错误和原生 Provider 操作／回合失败。
证据位于 daemon 数据目录的 `diagnostics/incident-<uuid>.txt`，包含故障 ID、UTC 时间窗口、
Ait 版本、系统架构、请求 ID／安全错误类别、Harness 版本与可用日志片段。
应用诊断附带当前采集与最近 3 份证据；更早且未过期的文件仍在该目录。

连续错误最多每 30 秒触发一次后台采集，同一窗口的错误共享证据。最近事件最多 128 项，
事件覆盖、采集队列丢弃及保存失败都有计数。报告缓存 30 秒；刷新过于频繁可能得到同一份。
后台维护会补采窗口内较晚到达的错误。故障发生后应尽快导出，以免被时效／容量上限淘汰。

保留上限同时为 **7 天、20 份、5 MiB**；单份最多 **256 KiB**。自动清理只作用于 Ait
自己生成的证据，不会删除 Harness 日志。导出的用户文件不在自动清理范围内。

## Harness 日志来源

| Harness | 默认来源 | 覆盖变量 |
| --- | --- | --- |
| OpenCode | 使用已配置的 executable 执行 `debug paths log` | `AIT_DIAGNOSTICS_OPENCODE_LOG_DIR` |
| Codex | `CODEX_HOME/log`，未设置时 `$HOME/.codex/log` | `AIT_DIAGNOSTICS_CODEX_LOG_DIR` |
| Claude | `CLAUDE_CONFIG_DIR/debug`，未设置时 `$HOME/.claude/debug` | `AIT_DIAGNOSTICS_CLAUDE_LOG_DIR` |
| DeepSeek Harness | 无默认目录，明确显示 unavailable | `AIT_DIAGNOSTICS_DEEPSEEK_HARNESS_LOG_DIR` |
| Antigravity | 无默认目录，明确显示 unavailable | `AIT_DIAGNOSTICS_ANTIGRAVITY_LOG_DIR` |

`AIT_SERVER_<PROVIDER>_BIN` 同时控制相应 CLI 版本探测；未设置时使用 PATH 中的
`opencode`、`codex`、`claude`、`dsh`、`agy`。环境变量在 daemon 启动时读取，修改后重启。
覆盖目录必须是专用日志目录，不应指向项目根目录或凭据目录。

最多扫描 1024 个目录项，选最近修改的 3 个 `.log` / `.txt` 普通文件，分别读取末尾
64 KiB。仅保留故障前 5 分钟到采集时刻之间的 ISO 时间戳行，时区偏移被统一为 UTC；
没有时区的时间按 UTC 解释。无时间戳的续行、会话 JSONL、符号链接不会被收集。
日志采用其他格式或本地无时区时间时，报告可能没有匹配记录，不能据此认定没有发生错误。

## 分享与排查

报告会删除常见凭据／提示词字段所在行、隐藏 URL、邮箱和常见用户目录；桌面日志在客户端
导出前再次脱敏。可先在诊断窗口检查内容，再分享附件。没有自动上传、账号授权或自动建 Issue。

若报告显示 unavailable / truncated / timeout，可据此判断缺失来源，而不是将其误认为
“检查通过”。进程立即崩溃／强杀、未写入日志的前端错误及仅输出到 stderr 的内容不保证
被保留。对于这类问题仍需平台崩溃报告或复现步骤。
