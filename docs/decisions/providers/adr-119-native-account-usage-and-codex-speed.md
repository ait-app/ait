# ADR-119：原生账号用量与 Codex 速度目录

- 状态：已接受
- 日期：2026-10-10
- 关系：延续 ADR-027 的会话端口与 Manager 生命周期边界、ADR-047 的 Plugin 移除，以及 ADR-091 的独立会话调度。

## 背景

Paseo 的新版用量界面支持固定窗口、切换已用或剩余百分比、主机选择和会话账号用量。AIT 已有 Rust 原生 Provider 用量查询；直接显示 daemon 的登录用量，会把通过临时环境启动的 Agent 误标为主机账号。Codex 的速度也需要按原生模型目录提供，原来的 Fast 布尔值无法表达 Ultrafast 或后续档位。

## 决策

1. 保留 `provider.usage.list.request` 与原有 `providers` 响应。新增可选 `agentId`、`providerId` 和 `forceRefresh`；旧的无参数请求仍读取主机各 Provider。前端将原生响应投影成共享用量卡片，不增加插件来源或凭据管理。
2. `AgentSession::account_usage` 是会话实际启动账号的只读端口。Codex 和 Claude 使用会话已有的客户端、工作目录和临时环境读取用量。默认实现报告不支持；停止或导入但尚未启动的会话报告不可用，不能替换成 daemon 的账号。
3. 带 `agentId` 的查询进入该 Agent 的执行通道，由其 live session 回答。主机查询仍使用独立读取通道；`providerId` 可以把单卡刷新限制到一个 Provider。无效参数、未知 Agent 或不匹配的 Provider 明确失败。
4. 主机报告在 `AgentManager` 的共享内存中缓存五分钟，最多保留 64 个 Provider，强制刷新绕过缓存。Agent 报告不使用主机缓存。前端查询键按主机与 Agent 分开，会话详情关闭后立即回收缓存，重新打开时读取当前账号。
5. 凭据仅由原生 CLI 管理。Claude 用量读取尊重有效的会话 OAuth/API-key 环境覆盖和配置目录；已过期或被 HTTP 401/403 拒绝的 OAuth 返回结构化问题及 `claude /login` 恢复提示。HTTP 错误不转发响应正文，网络或解析失败使用通用错误，不把 token、刷新凭据或原生错误体发送给界面。
6. Codex 速度从 `model/list` 的 `serviceTiers` / `additionalSpeedTiers` 推导，校验并去重后形成 `service_tier` select。Normal 始终存在，其他选项只有原生目录声明时才出现；不使用模型名称白名单。默认模型会话首次启动时通过现有原生连接读取目录，不依赖界面先触发模型发现，也不额外启动查询进程。桌面快速档位使用黄色闪电图标。
7. 原有 `fast_mode` 配置继续读取。当原生目录仅提供 `priority` 时映射 Fast 到该档位；显式 `service_tier` 优先。每次 `turn/start` 都发送当前档位，包括 Normal 的 `default`，避免继承上一轮的快速档位。未声明的档位在原生执行前被拒绝。

## 后果

- Provider 内部负责原生认证和配额语义，界面只保存显示模式、固定窗口和所选主机等设备偏好，不持久化账号凭据或 quota 数据。
- 后端仍由 Rust 实现，crate 依赖方向不变；共享 TypeScript 协议只声明显示与请求数据。
- 旧 daemon 支持基础主机用量，但新增的 Agent 作用域和单 Provider 刷新需要本次后端更新。
- 非活动会话无法展示当前账号用量；未声明速度档位的 Codex CLI 不会显示额外速度菜单。原生登录恢复仍需使用对应 CLI。
- 验证范围与平台限制记录在 [客户端移植报告](../../reports/clients/paseo-desktop-app-port-2026-10-10.md)。
