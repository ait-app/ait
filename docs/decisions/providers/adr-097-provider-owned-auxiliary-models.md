# ADR-097：Provider 自选辅助小模型

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
