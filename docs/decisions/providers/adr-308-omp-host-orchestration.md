---
id: ADR-308
status: accepted
date: 2026-11-26
amends:
  - providers/adr-248-omp-paseo-native-rpc.md
  - providers/adr-302-omp-command-ownership.md
paths:
  - crates/provider/core
  - crates/provider/runtime
  - crates/provider/omp
  - apps/mobile/src/composer
  - apps/mobile/src/panels
---

# ADR-308：OMP 的宿主编排与配置命令

## 背景

OMP 原生任务已能投影子 Agent，但未连接 Ait 的跨 Provider 编排。PR #612 的 Vibe 原型
没有合并；需要用当前的外部 OMP、统一子 Agent 生命周期和命令归属裁决落地。
Advisor 已有配置开关，slash 输入仍被拒绝，形成两个不一致的控制入口。

## 决策

- `AgentSession::command_config_patch` 是可选同步端口：adapter 在当前命令目录确认归属后，
  将配置命令转换为 `ConfigPatch`。runtime 传入最新持久配置，用现有 configure 路径验证、
  保存和发布；复用输入收据去重，不写模型消息、不打断当前回合。默认返回 None，其他命令不变。
- OMP 声明支持宿主注入的 Agent MCP。仅连接名为 `ait` 的 HTTP server，通过公开
  `set_host_tools` / `host_tool_call` 桥接到 Ait `tools/list` / `tools/call`；凭据只在内存，
  不写 OMP 全局配置或持久句柄。保留运行时注入的父 Agent 身份，跨 Provider 子 Agent
  仍由 Ait 自身创建和登记。原生 MCP 配置继续由 OMP 管理，不新增通用 MCP 配置能力。
- `/vibe` 仅在私有扩展加载成功且完整原生命令目录无同名声明时由 Ait 拥有。模式通过
  公开扩展 API 与 host-tool RPC 实现；不修改、捆绑或依赖定制 OMP。开启和退出均等待
  原生完成确认，再发布 `runtimeInfo.extra.vibe.enabled`；Goal 与 Vibe 不同时启动。
- Vibe coordinator 属于 OMP adapter：fast/good 分别解析原生 smol/task 角色，每个 worker
  由单一 actor 拥有外部 OMP session 与 writer。子项状态、时间线和 guide/stop 收据复用
  共享 Subagent 契约。worker 不继承父 Agent 的 Ait MCP 身份，也不递归加载 coordinator。
- worker 句柄存于父会话 metadata 的 `aitVibeWorkers`。退出、取消、连接失败和父身份切换
  停止旧 worker；恢复仅恢复可查看的历史身份，不自动重新启用 Vibe 或重发工作。
  registry、待启动项、并行调用、结果、通知及错误队列均有界。
- 客户端仅按确认快照显示 Vibe 胶囊，worker 仍使用现有子 Agent 胶囊。退出失败保留模式与错误，
  断线禁用操作。功能开关的开启图标使用各自语义色、关闭中性灰，快捷按钮和菜单共享规则。

## 后果

OMP 可通过 Ait 创建其他 Provider 的子 Agent，也可在 Vibe 中使用持久 OMP worker。
原生 task 子项继续走原有事件与历史链路；Shell 后台作业不冒充子 Agent。
宿主桥要求公开 host-tool RPC；不支持该 RPC 的 OMP 在连接或启用时明确失败，不伪装成模型提示词。
Advisor 命令和界面一样更新下一轮配置，当前原生回合不被强制重启。
