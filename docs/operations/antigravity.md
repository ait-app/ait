# Antigravity CLI

Ait 的 `antigravity` Provider 使用 Google 官方 AGY CLI，主会话通过官方 ACP（`agy acp`）运行，
支持 ACP 的旧版也可使用，推荐 **AGY 1.5.15 或更新版本**以获得后台任务、子代理与取消收尾修复。
仅安装 Antigravity IDE 不足以启动这个 Provider。

## 安装与登录

macOS / Linux 的官方安装：

```bash
curl -fsSL https://antigravity.google/cli/install.sh | bash
```

Homebrew：

```bash
brew install --cask antigravity-cli
```

Windows 的官方安装和企业认证配置见
[Google 安装文档](https://antigravity.google/docs/cli/install/)。
安装后先在终端运行 `agy`，完成原生登录，再在 Ait 选择 Antigravity。
Ait 使用 CLI 的缓存认证；登录信息不进入 Ait Provider snapshot 或 persistence handle。

额度刷新、创建/恢复会话和配置变更后的重连，会先用只读的 `agy models` 检查缓存认证。
检查失败时直接返回错误，避免启动可能打开浏览器的 `/usage` 或会话进程。
未登录或登录失效时，请手动在终端运行 `agy` 完成登录，再回到 Ait 重试。
该检查使用本次调用的原生环境和工作目录，不保存认证状态；重新登录后可直接重试。

daemon 默认检查 PATH，其次检查官方 `~/.local/bin/agy`、Windows 的
`%LOCALAPPDATA%\agy\bin\agy.exe`、`HOMEBREW_PREFIX/bin/agy`、
`/opt/homebrew/bin/agy`、`/usr/local/bin/agy` 与 Linux Homebrew 目录。
因此桌面应用的 PATH 未包含终端安装目录时仍能发现标准安装。

非标准安装可在启动 daemon 前指定路径：

```bash
export AIT_SERVER_ANTIGRAVITY_BIN="/custom/path/agy"
```

显式路径不可用时 Provider 显示 unavailable，不切换到其他安装。
不按版本号禁用 Provider：只读检查 `agy acp --help` 成功后可继续发现模型，具体能力由会话握手确认。
完全不支持 ACP 的 CLI 需要升级；命令无法启动时同时提示检查安装，避免把启动故障一律归因于版本。
Ait 不自动安装或升级 CLI。

## ACP 运行状态

主会话全部通过 `agy acp` 运行，不再保留 headless 主会话兼容路径。
旧版可继续多轮文本对话和其握手声明支持的功能。缺少图片能力时，只拒绝带图输入并提示升级，
纯文本仍可发送；缺少恢复能力时，当前会话仍可继续，聊天中提示恢复和配置切换需要升级。
配置切换因恢复能力缺失而被拒绝时，保留原连接和原配置，不丢弃仍可用的对话。
版本低于 1.5.15 或无法确认版本时，遇到 shell/子代理调用或取消操作会显示对应升级提示；
旧版可能提前结束后台任务所在轮次，因此其完成状态和用量可能不完整。提示只进入显示时间线，不注入模型输入。
已经观察到的未完成工具仍会触发失败收尾，不能将未确认完成的操作显示为成功。
无 `transport` 标记的旧会话 handle 会按原原生 conversation UUID 直接通过 ACP 恢复，不重放历史，
恢复后累计用量从新的 ACP 轮次起算。

1.5.18 的真实测试发现原生 ACP 不返回子代理 `conversation_id` 与 `log_uri`；
子代理执行本身正常，卡片只展示原生实际返回的信息。
结构化摘要生成仍使用隔离、禁止工具的 headless `--json-schema` 调用；
账号额度仍使用只读内置命令 `agy --print /usage --output-format json`，不增加轮询。
协议切换不承诺消除个人订阅的条款风险。

已录制 Linux x86_64 / AGY 1.5.18 的多轮、配置切换、关闭恢复、图片、文件工具、原生子代理交互，
脱敏后在 adapter 测试中离线回放；补充 smoke 确认普通模式审批拒绝、取消、shell 输出与退出码、
子代理写入文件及后台 shell 在关联 prompt 完成前结束。macOS 和 Windows 的真机验收尚未完成。
旧版兼容另实测 Linux x86_64 / AGY 1.5.13 与 1.5.15 的三轮文本对话和关闭后恢复；
这不表示 1.5.13 的后台任务和子代理收尾问题已修复。
录制测试默认跳过，需显式指定已登录的官方测试程序与私有录制输出路径；见
[原生 ACP 测试](../../crates/provider/antigravity/src/acp/tests/native.rs)和
[录制回放操作说明](provider-replay.md)。

## 执行模式与会话

| 模式                      | 原生行为                                                   |
| ------------------------- | ---------------------------------------------------------- |
| Local Permissions（默认） | 保留本机 AGY 权限规则；需要交互审批的工具由 adapter 拒绝   |
| Accept Edits              | 原生 `accept-edits` 模式，允许编辑；需要审批的命令仍被拒绝 |
| Plan                      | 原生 `plan` 模式                                           |
| Full Access               | `--dangerously-skip-permissions`，全部工具调用自动批准     |

普通模式的交互审批回调由 adapter 自动拒绝，不增加审批弹窗。
需要运行特定命令时，可在 AGY 自身配置权限规则，或显式选择 Full Access；Ait 不改写全局权限文件。

在沙箱化 Worktree（`sandbox: worktree_writes`）中，交互会话进程（含取消或切换配置后的重连）受外层
Seatbelt / `bubblewrap` 写入隔离约束：允许写入当前 worktree、Git 目录及 `~/.gemini/antigravity-cli`，
但该目录下的 `builtin`、`hooks.json`、`plugins`、`settings.json` 与整个 `~/.gemini/config` 保持只读；
启动参数自动追加不落盘的 `--settings '{"enableTerminalSandbox":false}'`，避免与外层沙箱嵌套冲突。

ACP 工具卡片保留原生状态和错误。交互审批被拒绝后，若原生以空响应完成，
Ait 将本轮显示为权限失败；已有响应且原生成功的轮次保留响应。
轮次结束时仍未收尾的工具会统一结束状态，避免卡片一直显示运行中。

AGY 的 stderr 与 RPC 错误会做有界分类诊断，仅公开权限、登录、额度、模型/API 或协议失败说明；
原始诊断不进入 Ait 日志或快照。更详细的原生日志位于 `~/.gemini/antigravity-cli/log/`，
incident 导出默认从该目录采集，可通过 `AIT_DIAGNOSTICS_ANTIGRAVITY_LOG_DIR` 覆盖。

模型和思考等级分开选择。`agy models` 把每个模型的每个 effort 各列一条记录
（如 `gemini-test-high` 与 `Gemini Test (High)`）；ID 以 `-<effort>` 结尾且标签以对应的
`(<Effort>)` 结尾的记录归并为一个模型，effort 成为该模型的思考等级选项，
按 low、medium、high、xhigh、max 排列。其余记录保持为单独的模型，没有思考等级。

ACP 根据原生配置选项设置模型、思考等级和模式，并复核整份最终快照；
若后续选项导致已选模型或等级变化，拒绝发送本轮输入并保留原配置。
模型列表不报告默认 effort，Ait 以该模型在原生列表中的第一条变体作为选择器默认思考等级。
所选模型不提供的等级在发送前拒绝，并提示重新选择。
分组之前保存的完整 slug（如 `gemini-test-high`）仍然有效：按模型加 effort 解析，
显式选择的思考等级优先于 slug 中的 effort；运行信息显示归并后的模型和实际使用的等级。

会话支持多轮文本和图片、工具进度、累计 token 用量与 conversation ID 恢复；
模型或模式修改在下轮开始前通过同一 conversation ID 重启原生连接。
读写、编辑、shell 和子代理步骤使用对应的工具卡片；原生没有返回写入内容或 diff 时，卡片只展示文件路径。

Ait 关闭或重启不会删除原生 conversation，已存 Ait 时间线也会保留。
需要在终端继续时使用会话页面的恢复命令：

```bash
agy --conversation <conversation-id>
```

取消发送 `session/cancel`，等待关联 prompt 的原生终态；确认后下一轮重新连接同一 conversation。
取消超时或进程提前退出时显示失败并清理活动轮次，下一条用户消息仍可重连。
原生错误、协议错误和进程崩溃同样会回收连接，避免重复结束事件；发送失败的消息不会自动重发。
Windows ACP 的真实取消行为尚未验证。关闭仍会回收拥有的原生进程。

## 账号额度

Ait 使用同一个 AGY 安装的 `agy --print /usage --output-format json` 读取原生账号额度。
主机用量使用 daemon 的登录环境；活动会话用量使用该会话的启动环境和工作目录，
不会替换为主机账号。主机查询缓存五分钟，手动刷新会绕过缓存。

用量卡片分别保留原生模型组的 5 小时和每周窗口、剩余百分比与重置时间。
这些窗口是账号的共享配额，与对话的累计 token 计数不同；不会从 token 计数推算余额。
原生响应没有账号名称或套餐时，Ait 不猜测这些信息。

可在同一主机终端执行上面的命令检查原生返回。该内置命令不发起模型轮次，
不创建 conversation；认证继续由 AGY 管理。没有额度窗口时显示不可用，
查询超时或响应不符合原生命令结构时显示读取失败，不转发原始错误体。
停止或尚未启动的会话仍需先启动或恢复，才能查询其当前账号。

验证和兼容范围见 [账号额度接入报告](../archive/reports/providers/antigravity-account-quota-2026-10-14.md)。

## 当前接口范围

文本、文本附件和 PNG/JPEG/GIF/WebP 图片支持。ACP 直接发送原生图片块，
同时将图片保存到 daemon 数据目录的私有 `agents/provider-images`，供历史消息预览和关闭后恢复；
不把图片写到工作区，也不更改原生权限。实际读图仍受所选模型能力和 AGY 原生权限约束。

单轮 JSON schema、system prompt、Ait 会话级 MCP 配置、
运行中 steer、完整原生历史导入/列举与 rewind 尚未接入。
不支持的输入与配置在发送前拒绝，保留具体输入错误原因；图片无法保存时提示检查磁盘空间和目录权限。
AGY 自身已配置的工具与 MCP 仍由原生 CLI 加载。

设计见 [ADR-087](../decisions/providers/adr-087-antigravity-cli-provider.md) 与
[ACP 迁移决策](../decisions/providers/adr-312-antigravity-acp-migration.md)，
验证见 [报告](../archive/reports/providers/antigravity-cli.md)。
