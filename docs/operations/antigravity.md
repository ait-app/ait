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

| 模式 | 原生行为 |
| --- | --- |
| Local Permissions（默认） | 保留本机 AGY 权限规则；headless 中无法询问的工具会被原生拒绝 |
| Accept Edits | `--mode accept-edits`，工具权限仍归本机 AGY 规则 |
| Plan | `--mode plan` |
| Full Access | `--dangerously-skip-permissions`，全部工具调用自动批准 |

Google 的 headless 接口没有交互审批回传。需要运行特定命令时，可按
[官方 Headless 文档](https://antigravity.google/docs/cli/headless)配置 AGY 自己的权限规则，
或显式选择 Full Access。Ait 不改写全局权限文件。

模型选择保留 CLI 返回的完整 slug，包括模型的 effort 变体。
会话支持多轮文本、工具进度、累计 token 用量与 conversation ID 恢复；
模型或模式修改在下轮开始前通过同一 conversation ID 重启原生连接。

Ait 关闭或重启不会删除原生 conversation，已存 Ait 时间线也会保留。
需要在终端继续时使用会话页面的恢复命令：

```bash
agy --conversation <conversation-id>
```

Unix 取消等待原生中断确认；确认后下一轮重新连接同一 conversation。
当前 Windows adapter 不支持取消。关闭仍会回收拥有的原生进程。

## 当前接口范围

文本和文本附件支持；图片、单轮 JSON schema、system prompt、Ait 会话级 MCP 配置、
运行中 steer、完整原生历史导入/列举、rewind 与 quota 查询尚未接入。
不支持的输入与配置在发送前拒绝。AGY 自身已配置的工具与 MCP 仍由原生 CLI 加载。

设计见 [ADR-087](../decisions/providers/adr-087-antigravity-cli-provider.md)，
验证见 [报告](../reports/providers/antigravity-cli.md)。
