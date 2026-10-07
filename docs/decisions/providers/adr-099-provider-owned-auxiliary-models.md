# ADR-099：Provider 自选辅助小模型

- 状态：Accepted。
- 日期：2026-10-07。
- 范围：标题、分支名与提交说明的辅助生成。
- 关系：修订 [ADR-058](adr-058-daemon-metadata-generation.md) 的候选模型规则，细化 [ADR-089](adr-089-provider-owned-composition.md) 的能力装配。

## 决策

`AgentClient` 显式声明辅助生成能力，并从自己的 discovery catalog 选择可用小模型。装配层按能力注册，不再单独列举 Codex 和 Claude。通用服务不维护 Provider 模型名单，也不沿用前台大模型。

`metadataGeneration.providers` 中的显式模型配置优先；只指定 Provider 时使用该 Provider 的小模型选择。前台选择只影响自动候选中的 Provider 优先级。禁用的 Provider 不参与，无法发现合适小模型时跳过该自动候选。显式模型覆盖不依赖 discovery 成功。

小模型的偏好及支持的低推理等级由 adapter 维护。发现使用逐 Provider 五秒超时，仍受辅助生成服务的整体预算约束。辅助请求使用已有原生认证，隔离前台会话并禁用工具。

本次接口变更迁移 Codex 和 Claude；OpenCode、DSH 的实际通道分别提交。Antigravity 不启用该能力。未提供通道的 Provider 不得声明支持。

## 验证

测试覆盖能力装配、禁用 Provider、显式覆盖、发现结果中的模型匹配、前台与辅助模型隔离、候选去重及失败回退。真实生成通道沿用各 adapter 的既有验证边界。

## DSH 通道

DSH 使用原生 headless profile，保留原生模型配置和认证。辅助调用先检查组合配置，关闭全部插件后仅启用模型和内存会话所需的核心插件；关闭工具实现、历史持久化、自动标题和启动注入。未知组合返回不可用，不回退到前台会话。输出限制为 1 MiB，调用受服务预算约束；取消时结束子进程，Unix 同时结束进程组。自定义模型插件不在允许列表内时不受支持。
## OpenCode 通道

OpenCode 辅助生成复用原生认证，启动私有 runtime 与一次性 agent，以单步和拒绝全部工具的配置覆盖执行。响应只接受已完成的 assistant 文本，拒绝工具条目；返回后删除临时原生会话并停止 runtime，取消时执行有界清理。V2 预分配会话 ID 以支持创建期间的取消清理；V1 若在取得创建响应前取消，或进程异常退出，可能残留原生历史，不承诺崩溃后的强制清理。
