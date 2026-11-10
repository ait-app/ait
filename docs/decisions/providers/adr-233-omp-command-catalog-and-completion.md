---
id: ADR-233
status: accepted
date: 2026-10-30
amends:
  - providers/adr-190-omp-acp-settlement.md
  - providers/adr-198-omp-interaction-progress.md
paths:
  - crates/provider
  - apps/mobile/src/composer
---

# ADR-233：OMP 会话命令目录与参数补全

- 状态：Accepted
- 日期：2026-10-30
- 修订：[ADR-190](adr-190-omp-acp-settlement.md) 的命令发现与
  [ADR-198](adr-198-omp-interaction-progress.md) 的文本交互映射。

## 背景

官方 OMP 的命令目录已经包含 aliases、source 和带说明、usage 的 subcommands。
Ait 原先只保留 name、description 和 input.hint，客户端在命令名后的空格处停止补全。
已有 Agent 的目录查询也使用临时原生进程，无法反映当前插件实例的目录变化。
客户端还会优先拦截 clear、new、exit、quit、q，与 OMP 的原生分发冲突。

## 决策

1. OMP adapter 规范化完整目录并验证别名、子命令和数量预算；共享协议增加可选的
   aliases、source、subcommands，其他 Provider 的旧响应继续有效。
2. Provider 显式声明是否支持会话目录，避免其他 Provider 的发现阻塞前台回合。
   AgentSession 可以提供自身最近观察到的命令目录。OMP Agent 的查询进入所属执行 lane，
   优先使用此目录；空目录同样权威。未运行的会话和草稿保留原有 Provider 发现回退，
   不为查询启动交互会话。带 draftConfig 的请求若解析到已有 Agent，同样使用所属 lane。
3. OMP 从同一实例的 available_commands_update 更新目录。ACP 按目录版本发布更新，
   完整补全信息放在命令的 \_meta 中；Ait 内嵌客户端从原生 owner 读取目录。
   不引入用于推断完成或重发输入的命令状态。
4. Composer 在根命令名后继续识别参数，按原生 subcommands 顺序与前缀筛选，展示说明和
   usage；支持别名、鼠标、方向键、Tab、Enter 和 Escape。补全只替换光标所在参数，保留
   后续内容与光标位置；第一个参数完成后释放 Enter 给原有发送链路。
5. OMP 会话的客户端命令让位给原生目录和分发，发送及队列入口都不拦截这些名称。
   其他 Provider 保留原有客户端命令规则。OMP 菜单重新打开时刷新会话目录。
6. 原生 input/editor 支持空字符串且保留原文；取消仍回传 cancelled，而非保存空答案。
   editor 的 prefill 映射为 initialValue；secret 输入使用密码控件。
   共享问答组件按显式标记处理这些能力，不匹配工具、插件或正文名称。

## 后果与边界

新增命令元数据均为可选字段，旧客户端可以忽略；测试夹具同时校验 Rust 适配、共享
schema、生成的 wire validator 与 Composer/表单行为。命令发现与更新不消耗模型 token。

这不等同于全部 OMP TUI 能力。官方 RPC 目录未导出扩展 getArgumentCompletions 函数、
subcommandOptional 和任意组件工厂；动态模型、MCP 参数及插件自定义补全不能仅凭静态目录
完整复现。Ait 不根据命令名硬编码这些规则。已有原生额外事件通道也不等同于 Ait 已实现
对应的 widgets、status、编辑器控制或所有原生会话操作界面。

来源：[官方 RPC 协议](https://github.com/can1357/oh-my-pi/blob/main/docs/rpc.md)、
[原生命令目录](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/slash-commands/available-commands.ts)、
[原生参数补全](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/slash-commands/builtin-completions.ts)。
验证与剩余差异见[兼容报告](../../archive/reports/providers/omp-commands-and-completion-2026-10-30.md)。
原始多级补全及完整操作入口的后续方案见[ACP 优先接入方案](../../plans/omp-acp-full-capabilities.md)。
