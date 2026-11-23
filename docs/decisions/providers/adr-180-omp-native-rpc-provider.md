---
id: ADR-180
status: accepted
date: 2026-10-21
paths:
  - crates/provider
  - crates/provider-omp
---

# ADR-180：Oh My Pi 原生 RPC Provider

- 修订：[ADR-190](adr-190-omp-acp-settlement.md) 曾将宿主入口改为 Ait ACP 桥接层，已被
  [ADR-248](adr-248-omp-paseo-native-rpc.md) 取代；Provider 重新直接驱动原生 RPC，本文的原生所有权保留。
- 修订：[ADR-194](adr-194-omp-settlement-reconciliation.md) 补充遗漏终态时的原生状态核对与用户取消期限。
- 修订：[ADR-248](adr-248-omp-paseo-native-rpc.md) 修改第 3 条的取消命令（`abort`）、第 4 条的帧限制（v1 回退、
  64 MiB 上限、容忍坏帧），并调整启动参数、特性、用量、provider options 与诊断。
  适配器现位于 `crates/provider-omp`（[ADR-249](adr-249-omp-adapter-crate.md)）。

## 背景

客户端已有 Oh My Pi 的图标、模式与恢复命令模板，但 Rust daemon 没有对应适配器，
选择 `omp` 的 Agent 无法启动。OMP 与已合入的 Pi Provider（[ADR-156](adr-156-pi-native-rpc-provider.md)）
是两个完全不同的独立实现：安装、原生配置、认证、会话目录和执行协议互不通用，
不能用一个 Provider 的名称或开关代表另一个。

协议依据为 OMP `18.12.5` 的
[RPC 文档](https://github.com/can1357/oh-my-pi/blob/d5cd750e41fede277f9eb933455da92a0738915e/docs/rpc.md)。

## 决策

1. `local/omp::OmpClient` 实现现有 `AgentClient` / `AgentSession`，独立维护启动、
   配置、传输、历史投影和测试；只使用其他 Provider 同样使用的通用图片存储与
   工具展示辅助函数。domain、公共 ports 和客户端协议不增加 OMP 分支。
2. composition 独立注册 `omp`，使用 `AIT_SERVER_OMP_BIN` 覆盖程序路径。
   程序通过 argv 启动，不经过 shell；保留原生工具、扩展和认证。
   私有环境覆盖只进入子进程，不进入持久配置。
3. OMP 运行 `--mode rpc-ui`，使用 `prompt_result` 和 `session_settled` 区分接纳、
   yield 和真正空闲，不把中间 `agent_end` 当作整轮完成；`session_settled` 仅在
   当前 prompt 的关联结果到达后才被接受，上一轮迟到的 settled 不会提前结束下一轮
   （含原生后台续跑）。取消使用 `abort_and_restore_queue`，输入不因通信失败自动重发。
4. ready 后协商协议 v2。分块按身份、顺序、数量、总长度和 base64 校验；
   限制物理帧 1 MiB、重组 64 MiB，拒绝交错或残缺序列。
5. 模型以 `[provider, modelId]` JSON 字符串标识，避免不同模型服务的同名模型冲突。
6. Ait 权限模式映射原生审批档位：`full`→`yolo`、`write`→`write`、
   `ask`→`always-ask`、`ask-cached`→`always-ask-cached`。原生 `ask`/`select`/`confirm`/
   `input` 扩展对话映射为 Ait 审批或结构化问答；原生子 Agent 生命周期与消息进入
   Ait 子 Agent 界面和各自时间线。
7. 原生历史直接读取 OMP 会话 JSONL：支持会话列表、导入、当前分支投影、
   空会话恢复；未写入原生历史的空 Agent 可以重新打开，已提交输入的历史文件
   丢失时返回错误，不创建替代对话或重发输入。回退、每会话 MCP/工具策略覆盖、
   输出 schema 与宿主工具注册不在本 ADR 范围。

## 影响

- OMP 与 Pi 的行为边界与限制见 [运维文档](../../operations/oh-my-pi.md)。
- 测试归 provider crate（[ADR-179](adr-179-provider-owned-tests.md)），
  使用离线 Python 协议夹具与真实 CLI 烟雾测试，daemon 保留跨能力组合测试。
