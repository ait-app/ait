# ADR-074：OpenCode 原生 Provider

- 状态：Accepted，2026-09-30。
- 范围：`bins/daemon` 与 `crates/provider`；取代已退役的原型执行路径（历史保存在 Git 中）。
- 上位约束：ADR-046、ADR-050、ADR-052、ADR-072。

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
4. 原生记录转换为 Paseo display items。原生消息 ID、客户端消息 ID 映射与 persistence
   handle 支持连续对话和重启恢复；历史读取不修改 permissions/model，也不提交输入。
   Ait 的 OpenCode 显示键带独立投影版本；升级时通过既有 reconcile 重建显示历史，原生消息不变。
   完成条目不可覆盖已有条目。Host admission、registry 与 timeline 事务继续归属 AgentManager。
5. Build 模式允许原生只读工具，shell/edit 请求交给现有审批接口。
   只接受具体可审查请求，所有 allow 均回复 native once；修改权限/输入的响应拒绝。
   撤回、取消与关闭清理待审批项。取消只有原生 interrupt HTTP 确认后才能成功。
   确认后继续核对完整 idle 历史，更新下一轮的预算基线，再释放当前 turn；
   无法核对时关闭 writer，不自动重发。已排空的取消允许后续 queued 输入继续执行。
6. 如原型，原生工具具有完整文件系统访问；native permission 不是 OS sandbox。
   模型与 reasoning variant 动态发现。当前只提供 Build，不宣称 plan/custom agents、
   steer、附件、表单、rewind、导入/列举外部会话、MCP 配置和后台任务已支持。
   不支持的配置和输入在提交前拒绝，避免默默丢弃。
7. v2 模型目录查询使用 `location[directory]` 编码。原生目录冷启动返回合法空数组时，
   只在五秒内重试读取；HTTP 错误、畸形响应以及非空但未启用的目录不自动重试。

## 验证

保留旧 18 项协议测试，新增 server port 的多轮、恢复、只读历史、客户端身份、审批与取消测试，
并以实际 `daemon` 二进制和离线 Python HTTP 子进程验证 WebSocket 执行及重启回收。
测量和未验证行为见 [报告](../../reports/providers/opencode-server-migration.md)。

## 后果

OpenCode 适配器沿用现有会话、审批和持久化边界。Codex 与共享 Timeline 保持上游实现。
原生协议兼容需要独立回归；显示投影升级会使旧 OpenCode 会话首次加载时重建游标。
最新验证与待完成的桌面复测见 [上游 PR 验证](../../reports/providers/opencode-upstream-pr.md)。
