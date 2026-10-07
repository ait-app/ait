# ADR-095：组件自行声明方法元数据

- 状态：Accepted。
- 日期：2026-10-07。
- 范围：`model` 方法类型、各功能组件与能力聚合、API 协议校验和审计脚本。
- 修订：[ADR-026](adr-026-canonical-paseo-websocket-surface.md) 中运行时中心方法目录、Paseo 名称映射和功能分组的决策；延续 [ADR-093](adr-093-consumable-request-context.md) 的组件分发和 [ADR-094](../clients/adr-094-canonical-ait-client-methods.md) 的 Ait 方法名。

## 背景

各组件已经声明自己实现的方法，功能 crate 根据实际服务安装组合能力。中心目录另行维护
方法名称、消息方向、Paseo 来源名称和 `MethodGroup`，形成重复清单，也使新增组件方法
需要在另一处补登记。实际业务分发不依赖该目录。

## 决策

安装粒度后由 [ADR-100](adr-100-crate-level-service-installation.md) 收敛为完整 crate 服务；
基础连接方法由 API 自行声明。下述细分安装组合描述本决策接受时的行为。

移除 `protocol::methods` 中心目录和旧名称查找接口。公共的 `MethodSpec { name, kind }`
及 `InboundKind` 放在 `model::methods`，各组件在原有方法声明处明确指定 request、event
或 response。能力 crate 仅通过 `implemented_methods` 和 `installed_methods` 提供完整声明
与按服务安装筛选的元数据，不再提供 `implemented_capabilities` 或 `installed_capabilities`
名称包装接口。API 聚合时统一提取名称；消息队列与事件分发直接使用方法元数据。

对外 `ServerInfo.capabilities` 仍表示可协商能力，`implemented_capabilities` 仍表示当前 host
已安装服务支持的能力，两者都从组件方法元数据派生。

API 汇总所有组件的元数据用于名称、消息方向和 capability 校验，拒绝重复声明，不再把
缺少中心记录的方法默认当成 request。可协商方法来自全部组件声明，已安装方法仍按 host
实际服务生成。未安装服务的方法可协商并返回 `not_implemented`；未协商返回
`unsupported_capability`，未知或旧名称返回 `method_not_found`。
`server.status.unsubscribe` 继续使用 `server.status.subscribe` 的现有连接级协商规则。

业务分发继续由所属功能 crate 自行组合，通过 `Option<Context>` 的消费与回退规则执行。
`protocol` 保留 envelope、版本与能力协商，不依赖具体业务组件，也不持有业务方法目录。
各组件只依赖现有公共 `model`，不增加向外的 crate 依赖。

固定 Paseo 快照移到 `scripts/fixtures/paseo`，只供离线审计与测试使用。脚本直接提取组件
方法声明，验证前端 method 和消息方向，并防止旧名称重新进入应用。

## 后果与验证

全部 179 个现有方法由所属组件声明，生产 host 仍发布 179 个已安装方法和单连接协议
capability。精简 host 也发布完整组件声明，未安装功能统一具有明确的占位响应，避免能力
协商范围依赖旧 Paseo 目录。新增方法只需要在所属组件声明并纳入本 crate 的安装组合。

测试覆盖元数据构造、每种服务安装组合、方法唯一性、event/response 方向、占位响应与
错误优先级、组件处理分支和生产能力安装。架构依赖守卫继续检查组件依赖方向；客户端与
审计脚本检查从组件声明提取的元数据。
