# ADR-082：Workspace 可空字段使用单级 Option

- 状态：Accepted
- 日期：2026-10-04
- 修订：ADR-065 的 Paseo wire fixture 输出逐字段一致要求

## 背景

Paseo 的部分 Workspace 和 Project 字段同时允许缺失、`null` 与具体值。Rust 的输出
结构为保持反序列化后逐字重序列化，使用 `Option<Option<T>>`。该表示扩散到目录投影和
Provider 构造代码，尽管当前 daemon 发送的是完整描述符，不需要重发旧消息中的字段缺失状态。
客户端协议接受这些字段的 `null` 值。

## 决策

- 对可缺失且可空的公开 Workspace、Project、脚本和运行时字段使用 `Option<T>`。
  反序列化时缺失和显式 `null` 都得到 `None`；序列化时 `None` 统一输出 `null`。
- 仍只允许缺失而不允许 `null` 的字段继续使用 `skip_serializing_if`。Checkout 的私有输入
  校验仍区分缺失与 `null`：Git 的缺失 worktree root 可以回退到 cwd，显式 `null`
  需要拒绝。这一三态解析不进入公开描述符或生产调用点。
- 固定的 Paseo fixture 继续验证输入是否合法和已存在字段的输出值；仅对这些已知可空字段
  允许 Rust 输出额外的 `null`。另行断言缺失字段会规范化为 `null`。

## 后果

Workspace 目录更新中没有运行时事实时，`diffStat`、`gitRuntime` 和
`githubRuntime` 等字段会显式为 `null`，以保持清空旧事实的行为。旧消息反序列化后
不再保留可空字段的缺失状态；重新序列化会补 `null`。受支持的客户端协议接受这两种
输入形式；需要通过测试维护此边界。此决策不改变字段名、请求方法或持久数据格式。
