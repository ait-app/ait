# ADR-212：由 Provider 拥有原生编辑器补全

状态：接受。日期：2026-10-30。扩展 [ADR-211](adr-211-omp-command-catalog-and-completion.md)。

## 背景

命令目录元数据不能表达插件动态函数、实时模型和 MCP 名称、嵌套参数或原生引用规则。
客户端猜测参数语法会与原生行为分叉。ACP 命令目录同样不能代替动态补全协议。

## 决策

- 增加只读 `agent.commands.complete.request`，输入完整 `text` 和 UTF-16 `cursor`。
  返回 `supported` 与原生顺序的 `items`，包含 ID、label、description、hint、精确
  `replaceRange`/`insertText`、可选结果光标及 `triggerCompletion`。
- 类型和大小验证归 provider crate，SDK 与 Composer 使用同一契约。输入至多 64 KiB，
  候选至多 256 项、合计 896 KiB；拒绝非法范围、重复 ID 和拆分代理对的光标。
- 已有会话在同一个执行 lane 由实际 native owner 回答，不新增历史写入者。草稿仅
  使用无持久化发现通道。其他 Provider 默认明确返回不支持，分别实现自己的原生映射。
- Composer 按完整输入、光标、Provider 和会话关联结果，延迟 120 ms 查询；输入改变
  立即撤下旧候选，接受前再比较快照。原生编辑原样应用，不补空格或改引用。原生空结果
  是权威结果；只有明确不支持才允许目录降级，错误不能伪装成降级成功。
- OMP 的 ACP 边界使用 `_omp/commands/complete`，返回后端真实支持状态。只有 RPC
  声明 `ready.capabilities.commandCompletion.version = 1` 才发送 `complete`。
  原版 OMP 18.14.0 不提供此能力，继续使用 ADR-211 的目录补全。
- 最小 OMP 上游提案放在 [third-party/omp](../../../third-party/omp/README.md)：从
  TUI 提取共享命令构建，ACP/RPC 调用同一补全器及插件函数。补丁不自动安装、不更改
  系统 OMP，也不代表官方已经发布该 API。

## 后果

查找不提交 prompt 或运行候选命令。生命周期、审批和取消仍归现有 owner。OMP 提案
将查询放在 RPC 后台控制队列，越过等待表单的命令，并设置 750 ms 查询期限。

远程入口只展示可执行命令。任意 TUI 工厂和带本地 `onApplied` 副作用的候选仍需单独
的语义动作协议。各 Provider 继续按源码和官方文档逐项验收，公共编辑契约不代表全部
功能已完成。
