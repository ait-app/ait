# ADR-190：Ait ACP 桥接官方 OMP 与通用插件接口

- 状态：Accepted
- 日期：2026-10-25

- 结算修订：[ADR-194](adr-194-omp-settlement-reconciliation.md) 补充原生空闲状态核对与有界取消。

## 背景

ACP 是固定入口，原有 RPC 功能必须保留，适配责任归 Ait。不能要求 OMP 修改源码、
安装专用构建或宣告 Ait 自定义扩展。官方 OMP 18.12.6 的 ACP 与 RPC 是独立启动模式，
另一个 RPC 进程无法观察 ACP 进程的内存任务；直接双进程不能补齐后台结算及子 Agent。

因此采用 Ait 自己的 Rust ACP→官方 RPC 适配层。用户输入最终也转译成原生 RPC，
不能将它描述为直连 OMP 自带的 ACP。此前附带 OMP 源码补丁的方案撤回。
本 ADR 修订 [ADR-180](adr-180-omp-native-rpc-provider.md) 的宿主入口，保留其原生所有权。

## 决策

1. OMP Provider 新建和恢复固定经过 Ait ACP 协议引擎，没有 Protocol 选择或直连 RPC 回退。
   旧 `transport: rpc/acp` 输入仅做兼容，句柄记录 `transport: acp`、`nativeTransport: rpc`。
   宿主内嵌调用与 `daemon omp-acp` JSONL stdio 使用相同的请求和事件引擎。
2. 每个正式会话由一个未修改的 `omp --mode rpc-ui` 进程持有对话、配置、审批、工具、
   MCP、插件、子 Agent、后台任务及历史。能力目录的无会话探测是独立临时实例，
   不将它的状态冒充正式会话状态，不并行双写原生历史。
3. ACP `initialize`、new/load/resume、prompt、cancel、steering、模式和 config options
   在 Ait 侧转换。prompt 响应必须等到原生关联结果及 settled，不在中间 agent_end 完成。
   输入不因超时或断开自动重发；原生拒绝和执行失败保留具体原因。
4. 以官方 ExtensionAPI 和 RPC 数据结构处理插件，不匹配插件名称、安装路径或特定命令。
   插件命令与模型动态发现，模型/推理选项取自原生事实；工具名称和参数不依赖白名单。
   `select/confirm/input/editor/ask` 通用映射审批或 form；可见 custom messages 在实时与
   JSONL 历史中共用投影，`display: false` 不进入展示。
5. 标准 ACP 无法表达的原生能力由桥接层协商 `clientCapabilities._meta.ompNative: true`：
   `_omp/rpc` 关联原生控制，`_omp/event` 保留事件字段，`_omp/respond` 回答属于该实例的回调。
   未知插件字段不丢弃，不为单个插件添加新分支。会改变对话所有权的原生生命周期命令
   保留给 ACP 路由；错误字段有界脱敏，回调验证身份及种类，取消或过期后不得复用。
6. Ait 内嵌客户端另协商 `_meta.ait: 1`，保留现有稳定 Timeline key、回执、用量、子 Agent
   和终态事件。仅为桥接序列化 provider 私有事件，不改变 domain 或公共客户端 wire。
   这些扩展由 Ait 提供，官方 OMP 不需要实现或声明它们。
7. 插件命令和原生控制可能先交互、后确认。输入写入后按请求 ID 异步接收原生确认，
   期间继续处理回调；本地命令的输出不能被吞掉，也不能等待确认而阻塞提问。
8. 原生支持的模型、推理和特性开关原地更新。CLI 绑定的模式/系统提示仍需重开时，
   未落盘空会话的 native ID 可以变化，ACP ID 保持不变。仅独立 ACP 连接需要时，
   Ait 在原生 sessions 同级 `ait-acp-aliases/` 原子保存自己的句柄与配置映射；恢复检查 cwd、
   大小和文件类型，不改写原生 JSONL，不保存认证信息。宿主连接仍使用原有持久句柄。
9. 官方安装、认证、MCP 与扩展配置继续归 OMP。Ait 不改写原生 JSONL、不复制用户登录秘密，
   不关闭本机插件、不复刻插件业务逻辑。原生图片 blob 按 hash、文件类型和大小校验后只读转换。

## 后果与限制

Ait 的 ACP 入口明确标识自身及 RPC 后端，不宣称使用 OMP 自带 ACP。
原生工具和插件只需安装在 OMP 中。桥接层提供协议能力，不能将 OMP RPC 本身未导出的
任意 TUI 组件工厂、主题或编辑器组件宣称为可远程渲染；具体通道、预算和限制见
[架构与兼容表](../../architecture/omp-acp-bridge.md)。

测试必须以未修改的官方 CLI 为基线，并使用不同插件名称验证共用路径。
定制版 OMP 的旧测试结果不能作为官方兼容证明；当前证据见
[验证报告](../../reports/providers/omp-acp-settlement-2026-10-25.md)。
