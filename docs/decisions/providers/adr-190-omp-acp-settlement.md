# ADR-190：OMP 必须使用 ACP 并承接原有 RPC 功能

- 状态：Accepted
- 日期：2026-10-25

## 背景

OMP 原生 RPC 使用 `session_settled` 区分暂时 yield 和整轮空闲。18.12.6 的原生 ACP
缺少等价的后台结算、特性配置和子 Agent 观测，且默认不加载本机 MCP。
将 ACP 作为可选协议、保留 RPC 默认值不能满足迁移要求。
本决策取代 [ADR-180](adr-180-omp-native-rpc-provider.md) 的执行协议部分；
`omp` 仍是独立 Provider，权限、工具、认证、扩展和历史归 OMP 原生实现所有。

## 决策

1. 新建、发现、草稿探测、命令与会话恢复全部使用 `omp --mode acp --no-pty`。
   删除 Protocol 控件和 RPC 执行适配器，不在能力不足时回退 RPC。
   旧 `featureValues.transport: rpc` 仅作为兼容输入接受，不能选择 RPC。
   旧 RPC 会话通过同一份原生历史恢复为 ACP，保留 session ID、模型、配置和消息回执；
   导入历史和后续持久句柄统一记录 ACP，不重发输入或新建替代会话。
2. 模型与推理、原有四个权限档位、自动压缩/重试和 fast/slow 开关通过 ACP
   `session/set_config_option` 配置。模型 ID 保持 `[provider, modelId]` JSON 标识。
   服务档位由当前原生模型的只读能力决定，切换模型后刷新；未支持的显式覆盖拒绝执行。
   系统提示变更时重新打开原生 ACP 会话，继续同一份历史。
3. ACP 原生扩展协商 `clientCapabilities._meta.omp: 1`，服务端宣告 `_meta.omp.version: 1`。
   `_omp/event` 携带原生消息/工具事件及当前会话拥有的子 Agent 事件；
   复用 OMP 原有的实时/历史投影，保留文本、推理、工具、图片、token/cost 与子 Agent 时间线。
   模型选项 metadata 保留每个模型的原生能力，命令 metadata 保留 skill 来源。
   公共 domain、ports、客户端 wire 与持久配置格式不增加新边界。
4. 正式会话显式协商 `_meta.ompNativeMcp: true`，由 OMP SDK 加载本机 MCP，
   空 ACP `mcpServers` 不再清空原生工具。普通 ACP 客户端继续隔离本机 MCP，
   不允许混合客户端 servers 与原生所有权。Ait 仍不提供每会话 MCP/工具策略覆盖。
   一次性探测使用 `--no-session`，不加载本机 MCP、不写特性设置或会话 JSONL。
5. 共享 ACP stdio、审批和 form 问答归通用 adapter 辅助模块。
   OMP 保留原有 8192 模型与 64 MiB 逻辑输出上限，不扩大其他 ACP Provider 的预算。
   历史和实时图片中的原生 `blob:sha256` 引用按严格 hash、文件类型和 32 MiB 上限读取，
   复制为 Ait 图片工件，不修改 OMP 文件。迟到工具进度不能重新打开已完成项。
6. 原生补丁通过 `_meta.sessionSettled` 协商版本 1，每次 prompt 携带 `_meta.turnId`。
   OMP 等待 streaming、已接纳输入、队列及能唤醒本 agent 的后台工作排空，
   在同一 ACP prompt 中转发后台续跑，持久化后发送匹配 session/turn 的 `_session/settled`。
   Ait 同时收到本轮成功响应和 settled 才结束运行状态，不以超时或没有输出合成完成。
   取消及原生错误具有独立终态；失败文本经有界脱敏，不能作为普通回复泄露凭据。
7. Host 终态前只读原生 JSONL，原子校正 Timeline 和用户回执。
   已提交历史丢失时明确失败；空会话可重新建立且不制造输入。
   OMP 原生配套补丁和测试归 [third-party/oh-my-pi](../../../third-party/oh-my-pi/README.md)。
   缺少必需扩展时在提交输入前拒绝，不降低功能、不回退 RPC、不自动替换系统 CLI。

## 验证

Provider 离线协议测试覆盖默认/旧配置强制 ACP、旧句柄恢复、原生特性、模型切换、
审批/问答、控制响应竞争、取消、追加输入、子 Agent、图片、用量、大输出、错误脱敏、
历史回执与后台结算。原生测试另验证普通 ACP 客户端兼容和本机 MCP 显式所有权。
实际命令、结果与未验证范围见 [报告](../../reports/providers/omp-acp-settlement-2026-10-25.md)。
