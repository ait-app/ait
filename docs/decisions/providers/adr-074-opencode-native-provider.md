# ADR-074：OpenCode 原生 Provider

- 状态：Accepted，2026-09-30。
- 范围：`bins/daemon` 与 `crates/provider`；取代已退役的原型执行路径（历史保存在 Git 中）。
- 上位约束：ADR-046、ADR-050、ADR-052、ADR-072。
- 2026-10-08：[ADR-110](adr-110-opencode-private-protocol-boundary.md) 细化双版本私有协议边界和权限拒绝结算，不改变公共 Provider 契约。

## 背景与参考

OpenCode 原型已迁入现有 Provider ports。本次适配上游 `6ce64504` 的 workspace 命名与文档分类，
使用 `bins/daemon` 与 `crates/provider`，不恢复已退役的 worker、CLI 或专用 crate。

对照本项目 `local/codex.rs`、Codex transport/streaming 与 `AgentClient` / `AgentSession`、
AgentManager 的输入接纳和 timeline 持久化。Claude Code 已通过 ADR-050/052 接入同一 port。
原生协议继续对照 Paseo `5599f9e567128a1240b3b15afab28bceef9d36a5` 的
`opencode/runtime-client.ts`、`server-manager.ts`、`v2/runtime.ts`、`session.ts`、
`turns.ts`、`history.ts`、`permissions.ts`，使用 1.14.46 / 2.0.10 协议夹具。

## 决策

1. `local/opencode::OpenCodeClient` 实现现有 `AgentClient`，Session 实现现有 `AgentSession`。
   daemon composition root 注册 OpenCode，可用 `AIT_SERVER_OPENCODE_BIN` 指定本机二进制。
   私有协议 DTO 不进入 domain，不增加全局 Provider 分支，也不读写 host 数据库。
   创建入口按已注册 adapter 校验 Provider，移除旧 Codex/Claude 固定白名单。
2. 每个交互连接持有独立 `opencode serve` 进程、随机端口和随机 Basic 密码。
   HTTP 限定 `127.0.0.1`，禁用代理与重定向。Unix 独立进程组由 Session 回收；
   observer 由取消令牌与 AbortOnDrop 管理，关闭有时限。认证仍由本机 OpenCode 管理。
3. start 先等待 SSE 就绪，再提交一次新输入；丢失响应只能核对历史，不重发。
   后台 observer 把有界进度与审批事件交给 daemon actor；每个文本项首次增量前，
   按原生历史发布其已完成的前序条目（用户、说明文字、工具与推理），完成后核对完整 timeline。
   前序条目尚未写全时，该回合余下输出等最终历史确认；不重发输入。
   v2 在全分页历史前后核对持久执行日志及 idle，未排空/不完整历史不能完成。
   2.0.20 的公开日志不返回执行事件时，以全分页历史末尾的持久 `idle` 记录为完成依据，
   校验其时间不早于最近用户输入且 outcome 与会话一致；旧回复或单独会话状态不能完成新输入。
   拒绝工具时 2.0.20 可能只持久化 `finish=error`、`error.type=aborted` 的助手记录，
   不写入 idle。仅当末条记录具有本轮创建/完成时间、且历史读取前后原生会话均不活跃时，
   将该回合结算为 interrupted；不补发 interrupt，也不重放输入。
4. 原生记录转换为 Paseo display items。原生消息 ID、客户端消息 ID 映射与 persistence
   handle 支持连续对话和重启恢复；历史读取不修改 permissions/model，也不提交输入。
   Ait 的 OpenCode 显示键带独立投影版本；升级时通过既有 reconcile 重建显示历史，原生消息不变。
   完成条目不可覆盖已有条目。Host admission、registry 与 timeline 事务继续归属 AgentManager。
5. Build / Plan 对应原生 agent，不再写入 Ait 固定权限策略。创建、导入、恢复和模式切换
   默认保留 OpenCode 的配置及会话权限；原生 allow / ask / deny 决定是否需要审批。
   2026-10-09 补充：通过既有 feature select 接入独立的原生会话权限入口，用户可显式选择
   allow / ask / deny，设置当前原生会话的通配规则；不增加自动回复审批的策略或新的 agent 模式。
   未选择时不改写权限，已选择时在下一轮提交前通过 native session API 应用并回读完整规则。
   v1 PATCH permission 追加单条规则；v2 PATCH permissions 保留原数组再追加规则并替换。
   新规则按原生顺序覆盖先前匹配项；这是显式修改原生会话规则，区别于审批 always 的保存规则。
   不更改用户全局/项目配置文件；规则保存在原生会话中，Ait 的选择通过既有配置契约持久化。
   重复回合/恢复不重复追加相同尾规则；权限写入失败或回读不符时不提交 prompt，关闭该 writer。
   shell/edit 及其他原生 action/resource 请求交给现有审批接口，保留原生规则范围。
   默认允许回复 native once；仅当原生请求提供非空的可保存规则时显示 always，必须由用户
   显式选择，并展示保存范围。未知动作、修改权限/输入的响应拒绝。
   撤回、取消与关闭清理待审批项。取消只有原生 interrupt HTTP 确认后才能成功。
   确认后继续核对完整 idle 历史，更新下一轮的预算基线，再释放当前 turn；
   无法核对时关闭 writer，不自动重发。已排空的取消允许后续 queued 输入继续执行。
6. 如原型，原生工具具有完整文件系统访问；native permission 不是 OS sandbox。
   模型与 reasoning variant 动态发现。支持 Build / Plan 及空闲回合间切换，不宣称 custom agents、
   steer、附件、表单、rewind、MCP 配置和后台任务已支持。
   不支持的配置和输入在提交前拒绝，避免默默丢弃。
7. v2 模型目录查询使用 `location[directory]` 编码。冷启动先等待原生 `/api/plugin` 发布
   初始激活批次的 inventory，再读取模型；部分非空模型目录本身不能证明配置已应用。
   空 inventory 或空模型目录只在五秒总预算内重试；HTTP 错误、畸形响应以及非空但未启用的目录不自动重试。
   不创建探测会话、不重载用户配置。后续远程插件安装和运行中配置变更仍需重新发现。

8. 通过既有 `AgentClient` 的列表和检查端口导入外部会话。v1 使用跨项目
   `/experimental/session`，v2 使用带游标的 `/api/session`；仅在用户指定目录时过滤，
   限制扫描数量、响应总字节和列表请求总时限。导入检查保留原生身份、标题、时间、模型和
   variant（原生 `default` 映射为未显式指定）；历史和模型凭据仍由 OpenCode 管理。
   既有 `resume_metadata` 保存恢复所需非秘密配置，兼容旧 Ait persistence handle。
   列表和导入不发送 prompt 或修改权限，保留原生 agent；不支持的自定义 agent 明确拒绝。
   未显式选择权限时，继续和恢复不追加或替换权限。显式权限选择按第 5 条应用。
   已有规则保留，新选择采用原生最后匹配优先语义；细粒度配置仍由 OpenCode 原生配置管理。

## 验证

保留旧 18 项协议测试，新增 server port 的多轮、恢复、只读历史、客户端身份、审批与取消测试，
并以实际 `daemon` 二进制和离线 Python HTTP 子进程验证 WebSocket 执行及重启回收。
测量和未验证行为见 [报告](../../reports/providers/opencode-server-migration.md)。

## 后果

OpenCode 适配器沿用现有会话、审批和持久化边界。Codex 与共享 Timeline 保持上游实现。
原生协议兼容需要独立回归；显示投影升级会使旧 OpenCode 会话首次加载时重建游标。
最新验证与待完成的桌面复测见 [上游 PR 验证](../../reports/providers/opencode-upstream-pr.md)。
