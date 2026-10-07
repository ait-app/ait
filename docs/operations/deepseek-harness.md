# DeepSeek Harness

安装并配置[官方 DeepSeek Harness CLI](https://github.com/deepseek-ai/deepseek-harness)，
确认 `dsh --profile web --no-open --host 127.0.0.1 --port 0` 可启动原生交互 Host。模型路由、凭据和 profile 配置由 Harness 管理。
Ait daemon 注册 `deepseek-harness`；客户端模型选择器显示 **DeepSeek Harness**，
模型和推理等级从本机 Harness 的实际配置目录发现。

默认优先从 PATH 启动 `dsh`。找不到 CLI 时，Ait 自动检查 DSH 桌面安装包：
Linux 会解析 PATH 中 `deepseek-harness` 的真实位置，并检查 `/opt/dsh-desktop-linux-bin`
和 `/opt/deepseek-harness`；macOS 检查 `/Applications` 与 `~/Applications` 下的 DSH 应用。
仅识别包身份为 `@deepseek-ai/dsh-desktop` 且包含 CLI 入口的安装包。
Linux 使用包内 Node；macOS 没有独立 Node 时以 `ELECTRON_RUN_AS_NODE=1` 启动包内运行时。
这些入口用于模型发现、会话导入/恢复、ACP 和辅助生成，不会打开 DSH 桌面窗口。
缺失或不完整的安装包仍报告不可用；安装或升级 DSH 后需要重启 Ait daemon 重新发现。

独立启动 daemon 时，可以指定 CLI 路径覆盖自动发现（不要指向桌面 GUI 可执行文件）：

```sh
export AIT_SERVER_DEEPSEEK_HARNESS_BIN=/absolute/path/to/dsh
```

创建示例：

```json
{
  "config": {
    "provider": "deepseek-harness",
    "cwd": "/absolute/workspace/path",
    "title": "Harness task"
  },
  "initialPrompt": "Read this repository and explain its architecture"
}
```

向 `agent.create.request` 发送此 payload。可省略模型沿用 Harness 默认值；
指定模型时使用 `provider.models.list.request` 返回的完整 `id`，不要把 opaque ID 改写成裸模型名。
`thinkingOptionId` 也应使用返回选项的 `id`；某些模型的空字符串表示 provider default。

权限菜单支持 Read only、Workspace write 和 Full access，原生权限配置在下一次输入前应用。
工具审批保留一次性允许或拒绝，question 支持单选、多选与自由文本，回答会完成原生待决请求。通过现有
`agent.permission.resolve.request` 回答；`agent.cancel.request` 取消当前工作。
服务重启后 `agent.resume.request` 恢复已登记 handle。原生模式读取展示历史时，从 DSH 的完整记录
恢复用户消息、助手消息和工具时间线；旧版本遗漏的用户气泡会在 daemon 重启后的首次历史加载时修复。
此过程只读，不重新发送用户输入。记录不完整时保留 Ait 现有历史并报告读取失败。
工具参数的空字符串按 DSH 原生规则转为空对象；非法 JSON 原样交给工具校验。

目前支持文本、附件、受 native capability 约束的图片输入、图片输出、工具生命周期、
上下文用量、DSH profile 配置的 MCP、审批、question、权限模式、取消、会话恢复与外部会话导入。
在工作区的导入会话列表选择 DeepSeek Harness，或通过 `provider.sessions.recent.list.request`
列举已有会话；指定 `cwd` 时只显示该目录，未指定时跨目录发现。列表过滤原生标记为空的 probe 和子智能体会话；旧 DSH 未缓存空白状态时，会话可能仍显示在列表中。
`agent.import.request` 只读检查完整历史，保留原生 ID、模型、推理档位和权限配置；
导入不发送 prompt、不创建原生会话。未结束的回合或不完整日志不能作为已完成历史导入。
导入后的 Ait 会话可在 daemon 重启后继续原生对话；DSH 自定义权限组合保持原样，不强制换成内置 preset。
暂不支持其他前端实时历史同步、steer、rewind、commands 和结构化输出约束。
原生 Host 不接受 Ait 每会话 MCP override；请在 DSH web profile 中配置 MCP。
显示按原生已落盘消息更新，不保证逐 token 输出。
原生模型发现只读目录，不再创建 probe session；显式 ACP 模式仍需打开会话进行发现。

旧 CLI 或需要 ACP 每会话 MCP override 时，可在 daemon 启动前设置：

```sh
export AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp
```

ACP 没有权限模式、question 和外部会话导入。默认不会因原生 Host 出错而悄悄回退 ACP；
原生 Host 创建的 handle 不能交给 ACP 恢复。旧 ACP handle 可由原生 Host 接续，并从原生记录恢复展示历史；显式 ACP 模式仍保留原有本地展示历史。

实现边界见 [ADR-082](../decisions/providers/adr-082-deepseek-harness-native-host.md)，
测试范围见[原生 Host 验证报告](https://github.com/KirisameLonnet/ait/blob/dc6cb1e01158ba14120e471b980ffe902ec7e09b/docs/reports/providers/deepseek-harness-native-host.md)。

## 导入列表预览

导入列表保留 DSH 原生标题、目录与活动时间，优先使用 turnOutline 的首尾用户 prompt。旧会话没有缓存摘要时，通过只读历史快照及分页补齐；不会提交消息或创建 Agent。预览规范化空白并限制为 300 个 Unicode 字符。单会话读取最多两秒、8 MiB / 100 页，全列表额外预算十秒；超限、损坏或只有图片而没有用户文本时保留原列表项，不伪造 prompt。
## 模型发现

Provider 模型菜单直接读取原生 Host 的 `session/modelCatalog`，不会为读取目录创建原生会话、选择模型或发送输入。菜单先列出内置权限预设，创建/恢复会话时仍由原生会话的实际权限目录校验选择。目录读取不再依赖默认会话能否成功初始化。CLI 启动、原生目录或认证错误仍可能使 Provider 显示错误，应结合展开后的错误文字和 DSH 版本诊断。
## DSH 0.2 权限目录

新版 Host 从 `permissionPresets/catalog` 提供可选权限，历史投影只保留当前值；Ait 兼容此协议及旧版内嵌选项。恢复桌面中打开的原生会话前，应先让 DSH 释放该会话的写入所有权；即使没有运行中的回合，桌面仍可能持有写入锁。只读导入不需要抢占写入所有权。验证范围见[权限目录兼容报告](../reports/providers/dsh-permission-catalog.md)。
