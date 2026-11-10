---
id: ADR-065
status: accepted
date: 2026-09-29
paths:
  - crates/api
  - crates/model
---

# ADR-065：Paseo 目录、时间线与会话 API 兼容行为

- 关联：ADR-031、ADR-032、ADR-038、ADR-039、ADR-040、ADR-046

## 背景

用户要求逐个对照本地 Paseo server API、移植测试并修复不兼容行为。对照版本为
`30178c4f58b67f8472901356e1484022bd835de0`（0.10.0-beta.1）。目录的数字偏移游标、
Workspace 固定 done 状态、时间线原始事件分页及聊天分叉边界均与当前上游不同。

## 决策

1. `model::pagination` 只拥有通用标量排序与 Base64URL keyset 游标。metadata/provider
   仍拥有各自字段、过滤规则和默认排序；model 不引用能力 crate，也不引入领域实体。
   游标保存排序条件、排序值和稳定身份，删除前页记录不会跳过后页记录。
   文本及最终身份排序使用 ICU4X，按进程 `LC_ALL`、`LC_MESSAGES`、`LANG` 选择 locale，
   对照 Node 的 `localeCompare`。英文、中文、瑞典语测试固定上游实际比较结果；
   不再用 Unicode 码点顺序代替语言排序。参见 [ICU Collator](https://docs.rs/icu_collator/2.3.1/icu_collator/struct.Collator.html)。
2. Workspace 状态桶归 metadata model。metadata 声明只读 `WorkspaceActivitySource` port，
   provider 通过现有 Agent registry 和 Timeline 子任务记录实现该 port。组合根注入共享实例，
   metadata 不读取 Provider 数据库或引用 provider。状态按 Workspace 身份及根 Agent 归属聚合，
   状态进入时间保存在各 Directory 克隆共享的投影缓存中；它不是新的持久状态权威。
3. Provider 时间线继续保存不可变的源事件。RPC 查询在读取时合并工具生命周期、相邻同消息
   的助手片段及相邻推理片段，保留完整源区间。分页限制按展示条目计算，游标只确认连续覆盖。
   请求中的 legacy `canonical` 仍可解析，但返回 `projected`，与当前 Paseo session 一致。
   搜索和分叉导出复用同一投影；分叉拒绝已扩展或被后续工具更新跨越的检查点。
4. 历史目录搜索由 provider 的查询层负责。每个关键词可匹配 Workspace、Agent、分支或项目名；
   词内缩写和有界拼写纠错不改变原有排序及游标。纠错距离按上游预算计算，采用带状动态规划。
5. `agent.finish.wait.request` 在审批待处理时返回 permission；超时返回 null lastMessage。
   显式正数 timeoutMs 不再受 30 秒上限限制，省略时等待结果或调用方取消。本条取代 ADR-032
   的“wait 默认/最大 30 秒”，连接级 32 个 wait 预算、断线取消、shutdown 回收和有界命令队列保留。
   已确认的取消仍等待原生 terminal event；取消中的旧审批不能提前结束这次等待。
   未知身份以内联 error 返回；正在等待的调用通过临时 observer 保留自动归档前的最后回复，
   observer 随等待结束释放，不为已经关闭的 Agent 保留原生资源。
6. `model::directory_sync` 只拥有可序列化投影的 generation、集合内序号和有界删除记录。
   metadata 与 provider 仍负责构造完整候选集和过滤。组合根注入同一目录 generation；
   Project、Workspace、Agent 的 headSeq 各自独立。连接拥有初始响应及后续观察者，
   每 250ms 读取并压缩变化；释放、断线或 shutdown 终止观察，失败关闭连接以触发重同步。
   新观察任务的登记与 shutdown 共用 admission 锁，不得在 tracker 关闭后登记。
7. `ServerInfo.features` 为既有方法的可选行为提供显式标记。完整目录服务安装时发布
   `directory-sync-v1`、`directory-subscriptions-v1`；SDK adapter 同时检查这些标记与所需方法，
   旧 server 没有标记时不启用新请求形状。此字段不授予方法调用权限，协议版本不变。
8. Provider 草稿功能发现使用 `AgentClient::draft_features` port，传入规范 cwd 与当前草稿，
   不注册 Agent 或提交输入。Codex 发现模型及工作流能力后返回适用功能；省略模型返回空列表。
   模型选择列表过滤 `isSelectable=false`，完整快照仍保留它们。全局目录缓存和显式 cwd
   目录缓存分开，空白 cwd 与缺省 cwd 使用全局作用域。
9. 显式 `agent.resume.request` 通过 `AgentManager::restore` 恢复可交互会话，可提供 overrides，
   原生验证和打开成功后才合并持久配置并清除归档。失败保留旧配置及归档，运行中输入不被丢弃。
   内部 `resume` 的历史用途仍为只读。本条取代 ADR-032 对显式归档恢复及 overrides 的阶段限制。
10. Agent 创建优先使用显式 Workspace 的 cwd，其次继承 caller Agent 的 Workspace 和 cwd，
    并写入真实 parent 标签。没有上下文的人类创建请求由 metadata 创建新 Workspace；
    幂等回执先于 Workspace 创建，因此重试不会产生重复 Workspace。此条取代 ADR-032
    对无 Workspace 创建及 caller 归属的阶段限制；Provider 不直接实现目录/工作树存储。
11. 未登记的原生 handle 可通过显式 resume 导入；先检查原生历史，再打开 Workspace，
    原生会话打开成功后才登记新 Agent。已有 Agent 的 cwd override 可与所属 Workspace 根目录不同，
    输入校验分别检查原生 cwd 和 Workspace 身份，避免已恢复的会话无法继续输入。
12. Agent 创建的 env 使用 SecretString 容器，只进入该会话的原生子进程；持久创建回执仅保存
    env 的规范 JSON SHA-256，以便重试比较而不保存明文。Codex/Claude 的实例克隆隔离环境；
    Claude 继续强制关闭嵌套 CLI 标记并启用 SDK 所需 checkpoint 设置。
13. Agent 的 autoArchive 在首次 completed、failed 或 cancelled 终态后触发；审批不触发。
    普通 Agent 复用显式归档规则。新建 worktree 的 Agent 归档其 Workspace 内的全部 Agent，
    关闭原生 writer 后，通过 metadata 所有的 WorktreeProvisioning port 清理受管理 checkout。
    仍被其他 Workspace 引用的目录保留，跨 Workspace 子 Agent 仅解除父关联；失败保留清理责任以便重试。
14. Agent、Workspace RPC 与 Schedule 共用同一 worktree 服务；提供 modern branch-off、checkout-branch、
    legacy worktreeName/createWorktree 和原目录 Git 切分支。后者要求干净目录，创建分支使用 --no-track。
    Provider 不执行 Git；文件系统 adapter 负责路径所有权和有界 Git 命令。新 Agent 注册失败时先
    关闭原生进程再清理 checkout；成功后启动共享 Workspace setup。GitHub/GHES PR 来源通过
    filesystem 的 forge port 解析 head/base/仓库身份，再检出 PR ref；不得强制覆盖已有分支。
    fork checkout 标记为不可信，setup 需显式批准；按上游配置独立 push remote/refspec，
    检出本身不执行 push。临时 ref 和失败 checkout 在 adapter 内清理。
15. terminal 拥有活动 tracker，metadata 通过既有 WorkspaceActivitySource 聚合多个所有者。
    POST /api/terminal-activity 仅接受回环来源和每终端随机 token，保留 Host/Origin 校验、限制请求体，
    不使用 URL 携带秘密。token 只传入 PTY 环境，进程退出后失效；列表和日志不返回它。
    活动从 running 到 idle 产生 sticky finished，needs-input 映射为 needs_input；可见心跳清除焦点终端
    的 attention，成功的 Ctrl-C/Esc 输入清除 working。terminal_attention_required 使用共享会话订阅
    与终端焦点抑制，重复报告不重复通知；terminal-activity-v1 显式告知 SDK 新事件已安装。
16. Workspace、Project 和 worktree 归档由 API 组合层协调：filesystem 的 begin_archive 标记
    归档并保留所有权证明，Provider 批量归档并关闭 Agent，terminal 和 metadata automation
    关闭所选 Workspace 资源，finish_archive 复核活动引用后删除 checkout。已经归档的记录
    仍可用于重试。filesystem 声明 WorktreeArchiveCleanup port，由 API adapter 注入共享
    terminal/automation 服务；自动归档亦经过此清理点，不引入反向依赖。
    setup 通过 Workspace 取消标记停止后台线程并等待回收，失败的子进程保持所有权；
    metadata 的 WorkspaceAutomationRuntime 新增按 Workspace 身份清理方法。
17. Provider 通过共享 SessionEvents 发布 agent_permission_request、agent_permission_resolved、
    agent.provider_subagents.update，持久状态变更先于发布；原时间线流继续独立工作。
    agent-session-events-v1 告知 SDK 对应会话事件生产者已安装。
18. 含初始 Agent 的 Workspace 创建由 API 组合层协调 metadata 与 provider，共用一个持久
    creation 回执和预留身份；先验证 Agent 输入，再准备 Workspace、setup、Agent 和首条输入。
    Provider cwd 按源目录的相对路径映射到新 checkout。并发重试加入同一操作，调用方断线
    不取消已接受的创建；失败仍返回已创建的 Workspace。仅当未登记 Agent 且原生关闭成功，
    才保存允许重试标记；已尝试的首条输入不会因相同幂等键再次提交。
19. 创建请求的 subscribe 只观察本次操作，进度实时发布，完成后自动释放；显式
    creation.subscribe.request 仍是连接持有的长期观察者。共享服务安装时发布
    creation-lifecycle-v1，SDK 同时核对所需方法后启用 Workspace 初始 Agent 和创建回执。
    操作观察者及最终创建响应不返回 subscriptionId；该字段仅属于显式长期观察者。
20. 创建 coordinator 从持久回执重建资源身份索引，跨键及跨创建种类拒绝重复认领。
    Directory 在任何创建副作用前拒绝已登记的 Workspace 身份；Provider 的只读 registry
    身份检查绕过原生执行队列，避免被正在启动的另一个 Agent 阻塞。完成回执重放和观察
    均检查对应资源及目录仍存在；这些检查不会重新创建资源，也不会解归档。
    Agent-ready 后的首条输入失败标记 outcomeUnknown，重试不会重复提交输入。
21. 创建已经产生资源后的 setup 和回执收尾使用 Runtime 的排队执行入口，继续复用原有
    单工作配额，不因另一短任务占用配额而放弃收尾。等待响应 shutdown，真正启动后才进入
    任务 tracker，并与关闭 tracker 共用 admission 锁；新请求的普通快速拒绝策略保留。

## 验证与边界

对照测试固定原始 Paseo revision 与源码 SHA-256；生成器执行上游测试断言并保存预期输出，
Rust 测试无需 Node、Paseo checkout 或真实付费 Provider。详见
[API 对照与验证报告](../../archive/reports/daemon/paseo-api-audit-2026-09-29.md)。

本次不恢复 ADR-045/047/061 已移除的 Hub、Chat、Loop、Plugin 或 relay；既有 Ait 认证、
协议 envelope 与能力协商继续由 transport adapter 适配。没有修改领域 Message 历史、
原生 Provider 历史权威或 SQLite 格式。
