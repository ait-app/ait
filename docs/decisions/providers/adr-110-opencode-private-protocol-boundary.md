# ADR-110：OpenCode 双版本私有协议边界

- 状态：Accepted，2026-10-08。
- 2026-10-09：传输与生命周期由 [ADR-115](adr-115-opencode-acp-provider.md) 修订为官方 ACP；下文保留当时的决策背景。
- 范围：`crates/provider/src/local/opencode`；细化 ADR-074，不修改 Provider ports、domain、RPC 或客户端协议。

## 背景

OpenCode 1.x 与 2.0.10+ 的认证变量、HTTP 路由、模型和审批字段、流式事件、分页历史及
完成证据不同。此前版本判断散布在会话生命周期、审批、辅助摘要和展示桥接中；同一中断
路由重复实现，辅助摘要还固定使用 v1 的 abort。权限拒绝被混同为主动取消时，会对已经
自行结束的 v2 回合再发 interrupt，与 ADR-074 的拒绝结算约定冲突。

## 决策

1. 启动时探测二进制版本并固定协议，不在失败后猜测协议，不跨协议重试，不重放输入。
   保留 `1.x` 与 `2.0.10+` 的接入门槛；这个门槛不是所有补丁版本均已真实验收的承诺。
2. `local/opencode/protocol` 独占 wire 兼容细节：认证和路由、请求编码、模型目录、审批
   编解码、SSE、完整分页历史、预算事实、原生会话列表和 durable outcome 核对。
   `Api.version` 仅在该模块内可见。启动进程只能使用协议提供的认证变量和健康检查路径。
3. 公共生命周期只调用版本无关的准备、提交、中断、历史、审批和设置接口；保留一套
   once-only admission、observer、取消、writer 关闭、预算和投影逻辑。版本枚举及 DTO
   不进入 domain、AgentManager、RPC 或 UI，不复制两套业务状态机。
4. 流式文本在协议层就产生最终 native item identity：v1 保留 part ID，v2 转为
   `assistantMessageID:0`。publication 和 Bridge 不再根据协议修正 identity；最终历史
   和流式文本使用同一个展示键，既有 projection 版本无需变更。
5. 已确认的 native permission reject 与主动取消是不同的 observer 结果。拒绝后先检查
   原生会话是否仍活跃：已经空闲则不补发中断；仍活跃且没有主动取消的原生确认时，调用
   唯一中断入口并要求 HTTP 确认。两者都必须核对本轮 input 的终态和完整历史后，才允许
   writer 接纳下一轮。旧历史、缺失本轮输入或不完整结算均不能释放 writer，也不能重放。
6. 前台取消、预算停止和辅助摘要清理共用一个有界中断入口：v1 abort、v2 interrupt。
   辅助摘要仍只使用隔离的禁用工具配置，不改写用户的原生权限配置。
7. 保留双协议回归，夹具拒绝错误版本的路由；增加公共生命周期不包含 wire 分支的边界
   检查。真实二进制测试使用独立 XDG 目录、临时工作区和 loopback 模型，不读取凭据或
   消费真实模型额度，不替换用户安装。

## 后果

协议升级的修改位置明确，业务生命周期不会因版本增殖。raw native JSON 只在私有适配器
内解析，公共 Provider 契约不变。v2 的旧 durable execution、持久 idle 和无 idle 的
aborted assistant 完成证据仍分别核对，不能用 session-level outcome 代替本轮证据。

真实验收按具体版本记录；模拟测试与行覆盖率不能替代未运行版本、操作系统或模型的验证。
当前测试使用 OpenCode 1.18.4 和官方 npm `@opencode/cli-darwin-arm64@2.0.20`。
