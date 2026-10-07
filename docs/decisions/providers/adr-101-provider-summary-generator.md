# ADR-101：Provider 拥有摘要生成能力

- 状态：Accepted。
- 日期：2026-10-07。
- 范围：辅助摘要生成接口、配置注入和跨组件消费。
- 修订：[ADR-058](adr-058-daemon-metadata-generation.md) 的生成端口归属与 [ADR-089](adr-089-provider-owned-composition.md) 的生成器组装接口。

本 ADR 的后续迁移已由 [ADR-102](adr-102-provider-metadata-independence.md) 完成：provider
不再依赖 metadata，共享契约与基础设施归 model。以下保留摘要生成迁移阶段的决策背景。

后续 [ADR-107](../daemon/adr-107-direct-imports-from-owning-crates.md) 移除 summary 的共享类型
重导出；生成接口继续归 provider，请求和结果类型直接从 model 导入。

## 背景

标题、分支名、提交信息和 PR 文案都通过原生 Provider 的辅助调用生成。此前 provider
实现生成器，却依赖 metadata 定义的 `MetadataGenerator`、daemon 配置端口和项目配置
存储。生成能力因此与 Workspace 元数据服务的契约及存储实现耦合。

本次先消除生成能力对 metadata 的依赖。provider 的 Workspace 放置、创建回执、会话
事件及其他依赖仍存在；不在本次迁移这些业务服务，也不删除 Cargo 的 metadata 依赖。

## 决策

1. provider 定义 `SummaryGenerator` 和 `SummaryConfiguration`，通过
   `Providers::summary_generator()` 组装有界生成实现。原生 `AgentClient` 的辅助调用
   统一命名为 `generate_summary()`，保留隔离、预算、重试、输出校验及原生进程清理。
2. `SummaryKind`、`SummaryRequest`、`SummarySelection`、`SummaryError` 和异步结果类型
   是生成端与消费者共用的输入输出，放在无能力 crate 依赖的 `model::summary` 中。
   provider 在自己的 summary 模块中导出这些类型；model 不持有生成器实现。
3. provider 通过 `SummaryConfiguration` 读取实时候选配置与项目文案风格。daemon
   适配现有配置存储、Git 根查找、`ait.json` 和旧 `paseo.json` 回退；provider 不导入
   metadata 的配置端口或存储实现。生成测试使用该配置端口的测试替身。
4. metadata 保留消费端 `SummarySource` 端口，Workspace 命名与 filesystem 的缺省文案
   通过它获取结果。API 的 `summary_source()` 将 provider 生成器适配到消费端口，daemon
   为 Workspace 命名注入同一生成实例；API 为 Git 消费与停机使用同一实例的适配器。
   适配器直接转发请求和取消，不新增缓存、任务、队列或锁。
5. provider 仍依赖 metadata，因此 metadata 不能反向依赖 provider。消费端口与宿主
   适配器保留单向编译依赖；生成器能力的定义和实现均归 provider。剩余依赖及迁移方向
   记录在[ADR-102](adr-102-provider-metadata-independence.md)。

## 后果与验证

生成代码、组装接口与 Agent 标题生成路径不再引用 `metadata::`，架构守卫检查这些源码。
共享请求字段、现有 `metadataGeneration` 配置键、客户端方法和四类生成结果保持兼容。
模型失效时仍按各消费场景回退，手工标题和明确提供的文案仍受保护。

定向验证覆盖配置热更新、项目根与旧配置回退、JSON 修复与候选回退、预算和取消、原生
调用隔离、Workspace 自动命名、Git 文案回退及 daemon 组装。共享适配器额外验证请求
字段传递和跨消费者取消。后续完全移除 provider 对 metadata 的依赖需逐项处理剩余边界。
