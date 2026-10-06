# DeepSeek Harness

安装并配置[官方 DeepSeek Harness CLI](https://github.com/deepseek-ai/deepseek-harness)，
确认 `dsh --profile web --no-open --host 127.0.0.1 --port 0` 可启动原生交互 Host。模型路由、凭据和 profile 配置由 Harness 管理。
Ait daemon 注册 `deepseek-harness`；客户端模型选择器显示 **DeepSeek Harness**，
模型和推理等级从本机 Harness 的实际配置目录发现。

默认从 PATH 启动 `dsh`。桌面启动环境找不到它时，在启动 server/desktop 前设置：

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
上下文用量、DSH profile 配置的 MCP、审批、question、权限模式、取消和会话恢复。
暂不支持原生会话导入、其他前端历史同步、steer、rewind、commands 和结构化输出约束。
原生 Host 不接受 Ait 每会话 MCP override；请在 DSH web profile 中配置 MCP。
显示按原生已落盘消息更新，不保证逐 token 输出。
模型发现会创建并关闭一个 Harness probe session；Harness 没有会话删除接口，
因此 probe 的原生持久化记录由 Harness 的保留策略管理。

旧 CLI 或需要 ACP 每会话 MCP override 时，可在 daemon 启动前设置：

```sh
export AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp
```

ACP 没有权限模式和 question。默认不会因原生 Host 出错而悄悄回退 ACP；
原生 Host 创建的 handle 不能交给 ACP 恢复。旧 ACP handle 可由原生 Host 接续，并从原生记录恢复展示历史；显式 ACP 模式仍保留原有本地展示历史。

实现边界见 [ADR-082](../decisions/providers/adr-082-deepseek-harness-native-host.md)，
测试范围见[原生 Host 验证报告](https://github.com/KirisameLonnet/ait/blob/dc6cb1e01158ba14120e471b980ffe902ec7e09b/docs/reports/providers/deepseek-harness-native-host.md)。
