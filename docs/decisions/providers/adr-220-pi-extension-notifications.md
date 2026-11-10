---
id: ADR-220
status: accepted
date: 2026-11-06
amends:
  - providers/adr-156-pi-native-rpc-provider.md
paths:
  - crates/provider
---

# ADR-220：Pi 扩展通知与会话时间线

- 范围：`crates/provider`；修订 [ADR-156](adr-156-pi-native-rpc-provider.md)。

## 背景

Pi 扩展会发出 method 为 `notify` 的 `extension_ui_request` 记录，其中包括那些不产生助手回复
的命令反馈。此前 Ait 会丢弃这些记录。把它们呈现出来会改变 provider 的会话投影，因此需要
明确的投递、持久化和资源边界，且独立于 [ADR-219](adr-219-pi-slash-command-discovery.md) 的
slash 命令发现。

## 决策

- 把活跃回合期间消费到的通知投影为现有持久化时间线中的 `notification` 条目，并使用适配器
  自有的唯一键。保留 warning 和 error 级别；其他级别映射为 info。包含原生 type 和
  notification type 元数据。`transient: true` 元数据描述的是原生通知本身，它不绕过宿主
  持久化。通知既不是助手回复，也不是回合失败，即使在 error 级别也是如此。
- 去除完整的 ANSI CSI/OSC 序列，把剩余的控制字符替换为空格，并省略空或纯空白的消息。通知
  类型元数据只在字符串不超过 128 字节时保留，否则记录为 null 并使用 info 级别。投影前丢弃
  大于 96 KiB 的消息。最多保留 16 条排队通知，额度按回合统计；需要时回收更早回合的通知。
  已发布的通知释放队列容量。通知不扣减助手文本预算，并把展示容量让给必需输出。提交的输入
  使用与输出相同的事件容量和终态清理预留；插入时先丢弃排队通知，若整批输入仍放不下则以原子
  方式失败。
- 投递是尽力而为的。stdout reader 在通道已满时丢弃通知，而不是阻塞响应路径。在控制请求期间，
  保留通知顺序并单独计量最多 16 条通知 / 2 MiB，使通知不会耗尽必需的事件容量。现有的原生帧
  限制先于解码生效。
- 只在消费通知时存在活跃回合才展示保留的通知；空闲通知被丢弃。保留现有的交互式扩展对话框
  取消行为，忽略其他受支持的仅通知 UI 方法，并拒绝未知 UI 方法。
- 使用现有的 provider 时间线事件和宿主存储；不引入新的 wire 协议、服务边界或原生历史重放
  保证。

## 后果与验证

用户可以在对话和保留的宿主历史中看到扩展命令反馈。压力下的通知投递有意允许丢失；通知不
保证完整的原生审计日志。原生负载仍受传输和展示限制约束。

Provider 归属的通知和传输测试覆盖投影、级别映射、ANSI 去除、空闲处理、控制响应顺序、通知
洪水和队列容量，包括重开时间线存储后的通知持久化。

运维与限制：[Pi provider](../../operations/pi.md)。
验证：[Pi commands and notifications](../../archive/reports/providers/pi-commands-notifications-2026-11-07.md)。
