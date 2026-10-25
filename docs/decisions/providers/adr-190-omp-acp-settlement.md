# ADR-190：OMP 可选 ACP 与原生后台结算信号

- 状态：Accepted
- 日期：2026-10-25

## 背景

OMP 原生 RPC 用 `session_settled` 区分 agent 暂时 yield 和整轮真正空闲。
18.12.6 的 ACP 只短暂排空已完成任务的投递，运行中的后台任务可能晚于 prompt 响应；
宿主不能把该响应或短暂没有输出视作 agent loop 完成。
本决策扩展 [ADR-180](adr-180-omp-native-rpc-provider.md)，为相同 Provider 增加 ACP 实现，
并同时修改外部 OMP 原生实现，补齐明确、可关联的后台结算信号。

## 决策

1. 继续使用 `omp` Provider，在新建草稿中通过 `featureValues.transport` 选择 `rpc` / `acp`；
   缺省为 RPC。协议写入恢复 metadata，已存在的会话不能中途切换协议。
2. ACP 使用 `omp --mode acp`、JSON-RPC stdio、原生 session/new / session/resume、
   流式更新、原生 model/thought_level/mode 配置、审批、form 问答、取消及 steering。
   模型 ID 继续使用现有 `[provider, modelId]` JSON 标识。
   OMP 拥有本地工具、认证和历史；Ait 不声明客户端文件或终端工具能力。
   原生 ACP 按客户端管理 MCP，此实现发送空 `mcpServers`，不加载本机 MCP 配置。
3. 通用 ACP 流投影和 form 问答分别归 `local/acp_stream`、`local/acp_interactions`，
   OMP、OpenCode、DSH 直接使用共享 adapter 辅助模块，不互相依赖具体 Provider。
   OMP 允许忽略已经完成或来自旧回合的工具进度，DSH 保持原有严格校验。
4. OMP 原生补丁通过 `_meta.sessionSettled` 显式协商版本 1；每次 prompt 携带 `_meta.turnId`。
   原生端等待 streaming、已接纳输入、队列和能唤醒本 agent 的后台工作排空，
   在同一 prompt 订阅中继续转发后台续跑，持久化历史后发送 `_session/settled`。
   最终后台续跑的结束原因决定 prompt 响应，保留原生错误详情。
5. Ait 仅在本轮成功响应与相同 session/turn 的 settled 都到达后完成整轮；
   过期信号不能释放新回合，不以超时或没有输出合成完成，也不自动重发已提交输入。
   取消及原生错误响应具有独立终态，不等待成功结算信号。
6. 结算前只读原生 JSONL，原子校正临时显示项和用户回执，再发布 host 终态。
   已提交会话的历史丢失时明确失败；未提交的空会话可以重新建立，不制造输入。
   后台工具命令失败作为 agent 的工具结果处理，不能因此关闭整个会话。
7. 新的 OMP 原生信号以可审查的上游补丁维护在
   [third-party/oh-my-pi](../../../third-party/oh-my-pi/README.md)，附带许可证与对应测试。
   未具备该扩展的原生 ACP 在提交输入前拒绝，不能悄悄降低后台执行语义。
   普通 ACP 客户端不启用扩展时保持原协议行为。
8. RPC 特有的 autoRetry / autoCompaction / fastMode / slowMode 不在 ACP 控件中宣称支持；
   草稿切协议时返回该协议的控件，让共享 UI 清理不再适用的覆盖值。
   执行入口仍拒绝无法应用的覆盖。domain、公共 ports、持久配置格式与客户端 wire 不增加新边界。

## 验证

Provider 中的离线协议夹具覆盖流投影、审批/问答、控制响应竞争、取消、历史恢复、
原生错误脱敏与持久化、后台信号关联及 manager 不提前关闭会话。
OMP 原生测试使用真实 agent loop 配合 mock 模型验证后台任务结果续跑，避免付费推理。
验证结果见 [报告](../../reports/providers/omp-acp-settlement-2026-10-25.md)。
