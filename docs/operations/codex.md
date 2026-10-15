# Codex

Ait 通过本机 Codex app-server 执行和恢复原生会话，认证与会话历史仍由 Codex 管理。

## 异步上下文压缩

在 Codex 会话的控制栏启用 **Async compaction**，允许原生 Codex 在后台压缩上下文。
新建会话时也可以在特性选项中选择；已有会话的修改在下一轮开始时生效。
正在运行的轮次不会因为切换开关而被中断，原生压缩完成事件继续显示在消息时间线中。

开关仅在当前 Codex 的 `experimentalFeature/list` 返回可用的 `async_compaction` 时显示。
未提供该能力、接口不可用或特性已废弃/移除时隐藏开关，普通会话继续可用。
如果看不到开关，先升级本机 Codex，再刷新 Provider 能力或重新打开会话。
Ait 根据原生能力查询结果判断支持情况，不以版本号作为门槛。

Ait 未设置该选项时继承 Codex 原生配置。选择开或关会保存为当前 Agent 的配置：

```json
{
  "featureValues": {
    "async_compaction": true
  }
}
```

高级配置也接受 `providerOptions.features.async_compaction` 布尔值；两者同时存在时，
会话控制栏的 `featureValues` 优先。Ait 将它转换为 `thread/start` / `thread/resume`
的 `config.features.async_compaction`，不改写用户的 `~/.codex/config.toml`。

压缩阈值、后台任务、摘要与后续消息合并、失败回退均由原生 Codex 负责。
Ait 不固定压缩阈值。手动 `/compact` 保持原有行为。

协议依据：[Codex App Server](https://learn.chatgpt.com/docs/app-server#list-experimental-features-experimentalfeaturelist)。
验证范围和本机版本限制见[验证记录](../reports/providers/codex-async-compaction.md)。
