# ADR-118：摘要接口归 model，配置文件适配归 persistence

- 状态：Accepted。
- 日期：2026-10-10。
- 范围：摘要生成契约、配置读取适配器和 daemon 组装。
- 修订：[ADR-101](../providers/adr-101-provider-summary-generator.md) 的接口与配置适配归属；
  延续 [ADR-112](adr-112-persistence-crate.md) 的文件实现边界及
  [ADR-107](adr-107-direct-imports-from-owning-crates.md) 的直接导入规则。

## 背景

摘要生成器实现位于 provider，但 `SummaryGenerator` 被 API 和 daemon 消费，
`SummaryConfiguration` 同时由生成器消费和文件读取适配器实现。两者定义在 provider，
使配置适配器无法移入仅依赖 model/domain 的 persistence。daemon 因此在
`host::summary` 中承载项目根查找、配置文件读取和旧文件回退等具体 I/O 逻辑。
此前一份未接入编译的 persistence 适配器草稿已存在，但依赖尚不存在的 model 接口。

## 决策

1. 将 `SummaryGenerator` 和 `SummaryConfiguration` 移入 `model::summary`，与消费端
   `SummarySource` 和 `SummaryFuture` 放在一起；请求、结果和错误值继续归
   `domain::summary`。model 不增加具体生成或文件实现。
2. provider 继续实现并组装有界生成器，负责候选模型、原生辅助调用、输出校验、预算和取消。
   provider、API、daemon 及测试直接从 model 导入接口，删除 provider 的 `summary` 模块。
3. daemon 的 `host::summary::Configuration` 由
   `persistence::storage::summary_config::LocalSummaryConfiguration` 取代。适配器通过共享
   `DaemonConfigStore` 读取当前全局配置，用 persistence 的项目配置实现读取文案偏好。
   构造函数接收现有存储的共享句柄，不复制配置快照。
4. 项目读取继续从 `cwd` 向上寻找最近带 `.git` 的目录；未找到时使用 `cwd`。优先读取
   `ait.json`，仅在文件不存在时回退 `paseo.json`；读取失败或缺失返回空偏好，由生成器
   使用默认风格。全局配置读取错误继续转换为 `SummaryError::Unavailable`。
5. daemon 只构造并注入 persistence 适配器，删除原配置适配模块及其重复测试。API 继续将
   同一生成器适配为 `SummarySource`，保留共享取消和停机生命周期。

## 后果与验证

依赖仍向内：persistence 和 provider 实现 model 契约，daemon 连接两者。不新增 Cargo
依赖；provider 的生产代码不依赖 persistence，persistence 不依赖 provider。配置键、生成
结果、默认风格和热更新行为不变。

配置热更新、项目根、旧配置回退和错误映射的测试随适配器迁至 persistence。定向验证覆盖
provider 的摘要生成、候选回退和 Agent 标题，API 的生成器适配，daemon 组装和 workspace
依赖守卫。
