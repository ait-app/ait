# Antigravity CLI

Ait 的 `antigravity` Provider 使用 Google 官方 AGY CLI，支持官方脚本与 Homebrew cask 安装。
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
模型列表从当前 CLI 的 `agy models` 获取；需使用支持官方 stream input/output 的较新版本。
本次真实验证使用 1.3.0。

## 执行模式与会话

| 模式                      | 原生行为                                                                |
| ------------------------- | ----------------------------------------------------------------------- |
| Local Permissions（默认） | 保留本机 AGY 权限规则；headless 中无法询问的工具会被原生拒绝            |
| Accept Edits              | `--mode accept-edits`，允许编辑；需要审批的命令仍会在 headless 中被拒绝 |
| Plan                      | `--mode plan`                                                           |
| Full Access               | `--dangerously-skip-permissions`，全部工具调用自动批准                  |

Google 的 headless 接口没有交互审批回传。需要运行特定命令时，可按
[官方 Headless 文档](https://antigravity.google/docs/cli/headless)配置 AGY 自己的权限规则，
或显式选择 Full Access。Ait 不改写全局权限文件。

工具被自动拒绝时，AGY 1.3.0 可能返回 `SUCCESS`、空响应和 `denied_actions`，
且工具完成事件不带 `error`。Ait 根据拒绝列表将对应工具显示为失败；没有响应的拒绝轮次
也显示失败，并在会话错误和活动日志中提示配置 `permissions.allow` 或选择 Full Access。
已有响应且原生成功的轮次保留响应，对被拒绝工具仍显示失败。
AGY 1.2.13 的工具 `ERROR` 步骤也会保留原生错误。`denied_actions` 是进程级累计列表，
Ait 只将新增拒绝用于本轮判断；重连后重新计数，避免旧拒绝让后续成功工具显示失败。

AGY 的 stderr 会持续、有界地读取，仅公开权限、登录、额度、模型/API 或协议失败的分类说明；
原始诊断不进入 Ait 日志或快照。更详细的原生日志位于 `~/.gemini/antigravity-cli/log/`。

模型选择保留 CLI 返回的完整 slug，包括模型的 effort 变体。
会话支持多轮文本和图片、工具进度、累计 token 用量与 conversation ID 恢复；
模型或模式修改在下轮开始前通过同一 conversation ID 重启原生连接。
读写、编辑和子代理步骤使用对应的工具卡片；子代理保留原生 child conversation ID 和日志 URI，
不导入子代理 transcript。原生没有返回写入内容或 diff 时，卡片只展示文件路径。

Ait 关闭或重启不会删除原生 conversation，已存 Ait 时间线也会保留。
需要在终端继续时使用会话页面的恢复命令：

```bash
agy --conversation <conversation-id>
```

Unix 取消等待原生中断确认；确认后下一轮重新连接同一 conversation。
支持 `ERROR` / `interrupted` 形式的原生确认。取消超时或进程提前退出时显示失败并清理活动轮次，
下一条用户消息仍可重连。原生错误、协议错误和进程崩溃同样会回收连接，避免重复结束事件；
发送失败的消息不会自动重发。
当前 Windows adapter 不支持取消。关闭仍会回收拥有的原生进程。

协议回归样本对照 [Paseo `dc07efe937c3`](https://github.com/getpaseo/paseo/tree/dc07efe937c3160e8fe83d02dc972c8a0b014957/plugins/antigravity-provider)
（2026-10-27 检查的最新 main），涵盖其 AGY 1.2.13 原始录制与 Ait 的 1.3.0 合成场景。

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

验证和兼容范围见 [账号额度接入报告](../reports/providers/antigravity-account-quota-2026-10-14.md)。

## 当前接口范围

文本、文本附件和 PNG/JPEG/GIF/WebP 图片支持。AGY 的流式输入只接受文本块，
Ait 将图片保存到 daemon 数据目录的私有 `agents/provider-images`，通过文件路径交给
原生读图工具；不把图片写到工作区，也不更改原生权限。历史消息保留图片预览，
关闭会话后图片仍可用于历史和恢复。实际读图仍受所选模型能力和 AGY 原生工具权限约束。

单轮 JSON schema、system prompt、Ait 会话级 MCP 配置、
运行中 steer、完整原生历史导入/列举与 rewind 尚未接入。
不支持的输入与配置在发送前拒绝，保留具体输入错误原因；图片无法保存时提示检查磁盘空间和目录权限。
AGY 自身已配置的工具与 MCP 仍由原生 CLI 加载。

设计见 [ADR-087](../decisions/providers/adr-087-antigravity-cli-provider.md)，
验证见 [报告](../reports/providers/antigravity-cli.md)。
