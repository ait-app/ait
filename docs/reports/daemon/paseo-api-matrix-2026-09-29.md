# Paseo Server 逐接口索引

由 `scripts/paseo-api-audit.mjs` 从本地 Paseo `30178c4f58b67f8472901356e1484022bd835de0` 的真实 Zod union 生成。
205 个入站名称，34 个已明确移除，171 个有效名称归并为 168 个 canonical 方法。
完整嵌套字段、每项 schema 指纹、上游分派位置和关联 Rust 测试保存在
[契约快照](../../../scripts/fixtures/paseo/paseo-api-contracts.json)；测试索引不是逐项语义覆盖率。

[修复、验证与未覆盖范围](paseo-api-audit-2026-09-29.md)。表格不以“有路由”推断完全兼容。

| Paseo 入站名称 | Ait 方法 | 上游输入字段 | Rust 入口 | 对照结果 / 限制 |
| --- | --- | --- | --- | --- |
| `abort_request` | `voice.abort.request` | — | `crates/voice/src/connection/voice.rs:123`<br>`crates/voice/src/dispatch.rs:28` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `agent_permission_response` | `agent.permission.resolve.request` | agentId, response | `crates/provider/src/rpc/agent_execution.rs:71`<br>`crates/provider/src/rpc/agent_execution/controls.rs:48` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.config.apply.request` | `agent.config.apply.request` | agentId, config | `crates/provider/src/rpc/agent_execution.rs:99` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.create.request` | `agent.create.request` | idempotencyKey, config, env, workspaceId, callerAgentId, worktreeName, initialPrompt, clientMessageId, outputSchema, images, attachments, git, worktree, autoArchive, labels, agentId, subscribe | `crates/provider/src/connection.rs:117`<br>`crates/provider/src/dispatch.rs:155` | 补齐 env、caller、worktree、原目录 Git、autoArchive、setup、GitHub PR checkout 和操作期间实时进度 |
| `agent.detach.request` | `agent.detach.request` | agentId | `crates/provider/src/rpc/agent_runtime.rs:46` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.fork_context.request` | `agent.fork_context.request` | agentId, boundaryCursor, boundaryMessageId | `crates/provider/src/rpc/agent_execution.rs:79`<br>`crates/provider/src/rpc/agent_execution/native_sessions.rs:50` | 修复投影边界验证、工具摘要和子任务日志 |
| `agent.provider_subagents.list.request` | `agent.provider_subagents.list.request` | parentAgentId | `crates/provider/src/rpc/agent_execution.rs:72`<br>`crates/provider/src/rpc/agent_execution/controls.rs:59` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.provider_subagents.timeline.get.request` | `agent.provider_subagents.timeline.get.request` | parentAgentId, subagentId, direction, cursor, limit | `crates/provider/src/rpc/agent_execution.rs:73`<br>`crates/provider/src/rpc/agent_execution/controls.rs:60` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.rewind.request` | `agent.rewind.request` | agentId, messageId, mode | `crates/provider/src/rpc/agent_execution.rs:69`<br>`crates/provider/src/rpc/agent_execution/controls.rs:33` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.skills.get_status.request` | `agent.skills.get_status.request` | — | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.skills.import_legacy_selection.request` | `agent.skills.import_legacy_selection.request` | selection | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.skills.reconcile.request` | `agent.skills.reconcile.request` | — | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.skills.save_selection.request` | `agent.skills.save_selection.request` | selection, confirmedRemovals | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.skills.uninstall.request` | `agent.skills.uninstall.request` | — | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.timeline.append.request` | `agent.timeline.append.request` | agentId, item | `crates/provider/src/dispatch.rs:169` | 仅接受带受信插件来源的显示项；不恢复已移除的 Plugin 产品能力 |
| `agent.timeline.list_prompts.request` | `agent.timeline.list_prompts.request` | agentId | `crates/provider/src/rpc/agent_execution.rs:93` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `agent.timeline.search.request` | `agent.timeline.search.request` | agentId, query, cursor | `crates/provider/src/rpc/agent_execution.rs:92` | 修复按相同投影搜索并定位 seqEnd |
| `agent.timeline.set_subscription.request` | `agent.timeline.set_subscription.request` | agentIds | `crates/provider/src/dispatch.rs:166` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `archive_agent_request` | `agent.archive.request` | agentId | `crates/provider/src/rpc/agent_runtime.rs:44` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `archive_workspace_request` | `workspace.archive.request` | workspaceId | `crates/api/src/connection/workspace_archive.rs:15`<br>`crates/metadata/src/rpc/directory.rs:59` | 修复 Agent、终端、setup/script 清理及共享 checkout 保留和失败重试 |
| `audio_played` | `voice.audio.played` | id | `crates/voice/src/connection.rs:105` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `branch_suggestions_request` | `checkout.branch.suggestions.request` | cwd, query, limit | `crates/filesystem/src/rpc/checkout.rs:21` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `browser.automation.execute.response` | `browser.automation.execute.response` | payload | `crates/api/src/connection.rs:210` | 核对输入与路由；由登记的浏览器 host 执行命令 |
| `browser.host.register.request` | `browser.host.register.request` | hostKind, supportedCommands | 见对应 capability 分派 | 核对输入与路由；由登记的浏览器 host 执行命令 |
| `cancel_agent_request` | `agent.cancel.request` | agentId | `crates/provider/src/rpc/agent_execution.rs:102` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `capture_terminal_request` | `terminal.capture.request` | terminalId, start, end, stripAnsi | `crates/terminal/src/rpc.rs:52` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `chat/create` | 已移除 | name, purpose | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `chat/delete` | 已移除 | room | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `chat/inspect` | 已移除 | room | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `chat/list` | 已移除 | — | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `chat/post` | 已移除 | room, body, authorAgentId, replyToMessageId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `chat/read` | 已移除 | room, limit, since, authorAgentId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `chat/wait` | 已移除 | room, afterMessageId, timeoutMs | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `checkout_commit_request` | `checkout.commit.request` | cwd, message, addAll | `crates/filesystem/src/dispatch/metadata.rs:12`<br>`crates/filesystem/src/rpc/checkout.rs:24` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout_merge_from_base_request` | `checkout.merge_from_base.request` | cwd, baseRef, requireCleanTarget | `crates/filesystem/src/rpc/checkout.rs:26` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout_merge_request` | `checkout.merge.request` | cwd, baseRef, strategy, requireCleanTarget | `crates/filesystem/src/rpc/checkout.rs:25` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout_pr_create_request` | `checkout.pr.create.request` | cwd, title, body, baseRef | `crates/filesystem/src/dispatch/metadata.rs:20`<br>`crates/filesystem/src/rpc/forge.rs:19` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout_pr_merge_request` | `checkout.pr.merge.request` | cwd, mergeMethod | `crates/filesystem/src/rpc/forge.rs:20` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout_pr_status_request` | `checkout.pr.status.request` | cwd | `crates/filesystem/src/rpc/forge.rs:21` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout_pull_request` | `checkout.pull.request` | cwd | `crates/filesystem/src/rpc/checkout.rs:27` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout_push_request` | `checkout.push.request` | cwd | `crates/filesystem/src/rpc/checkout.rs:28` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout_status_request` | `checkout.status.get.request` | cwd | `crates/filesystem/src/rpc/checkout.rs:15` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout_switch_branch_request` | `checkout.branch.switch.request` | cwd, branch | `crates/filesystem/src/rpc/checkout.rs:22` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout.commits.file_diff.request` | `checkout.commits.file_diff.request` | cwd, sha, path | `crates/filesystem/src/rpc/checkout.rs:19` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout.commits.list.request` | `checkout.commits.list.request` | cwd | `crates/filesystem/src/rpc/checkout.rs:18` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout.diff.get.request` | `checkout.diff.get.request` | cwd, compare | `crates/filesystem/src/rpc/checkout.rs:17` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout.discard_changes.request` | `checkout.discard_changes.request` | cwd, paths | `crates/filesystem/src/rpc/checkout.rs:29` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout.forge.get_check_details.request` | `checkout.forge.get_check_details.request` | cwd, repoOwner, repoName, checkRunId, workflowRunId, changeRequestNumber | `crates/filesystem/src/rpc/forge.rs:26` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout.forge.set_auto_merge.request` | `checkout.forge.set_auto_merge.request` | cwd, enabled, mergeMethod | `crates/filesystem/src/rpc/forge.rs:23` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout.github.get_check_details.request` | `checkout.github.get_check_details.request` | cwd, repoOwner, repoName, checkRunId, workflowRunId, changeRequestNumber | `crates/filesystem/src/rpc/forge.rs:27` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout.github.set_auto_merge.request` | `checkout.github.set_auto_merge.request` | cwd, enabled, mergeMethod | `crates/filesystem/src/rpc/forge.rs:23` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `checkout.refresh.request` | `checkout.refresh.request` | cwd | `crates/filesystem/src/rpc/checkout.rs:16` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `checkout.rename_branch.request` | `checkout.rename_branch.request` | cwd, branch | `crates/filesystem/src/rpc/checkout.rs:23` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `clear_agent_attention` | `agent.attention.clear.request` | agentId | `crates/provider/src/rpc/agent_runtime.rs:47` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `client_heartbeat` | `session.heartbeat` | deviceType, focusedAgentId, focusedTerminalId, lastActivityAt, appVisible, appVisibilityChangedAt | 见对应 capability 分派 | 补齐有效可见终端焦点的 attention 清除与通知抑制 |
| `close_items_request` | `agent.items.close.request` | agentIds, terminalIds | `crates/provider/src/dispatch/agent_runtime.rs:7`<br>`crates/provider/src/rpc/agent_runtime.rs:48` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `create_agent_request` | `agent.create.request` | idempotencyKey, config, env, workspaceId, callerAgentId, worktreeName, initialPrompt, clientMessageId, outputSchema, images, attachments, git, worktree, autoArchive, labels | `crates/provider/src/connection.rs:117`<br>`crates/provider/src/dispatch.rs:155` | 补齐 env、caller、worktree、原目录 Git、autoArchive、setup、GitHub PR checkout 和操作期间实时进度 |
| `create_paseo_worktree_request` | `workspace.worktree.create.request` | cwd, projectId, worktreeSlug, nameContext, attachments, firstAgentContext, refName, action, checkoutSource, githubPrNumber | `crates/filesystem/src/rpc/worktrees.rs:39` | 修复 checkout 默认目录名及比较基线；支持 GitHub/GHES PR、fork 推送配置与 setup 信任门控 |
| `create_terminal_request` | `terminal.create.request` | cwd, workspaceId, name, agentId, command, args, size | `crates/terminal/src/rpc.rs:33` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `creation.subscribe.request` | `creation.subscribe.request` | kind, idempotencyKey, subscribe | `crates/api/src/connection/dispatch.rs:14` | 连接所有权观察、断线恢复；完成回执查询校验目录和 Agent 仍存在 |
| `daemon.config.reload.request` | `daemon.config.reload.request` | — | `crates/metadata/src/connection/daemon.rs:21`<br>`crates/metadata/src/rpc/daemon.rs:100` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `daemon.get_pairing_offer.request` | `daemon.get_pairing_offer.request` | — | `crates/metadata/src/rpc/daemon.rs:78` | 按 ADR-061 保留禁用配对/relay 的响应 |
| `daemon.get_status.request` | `daemon.get_status.request` | — | `crates/metadata/src/dispatch.rs:155`<br>`crates/metadata/src/rpc/daemon.rs:75` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `daemon.update.request` | `daemon.update.request` | — | `crates/metadata/src/rpc/daemon.rs:110` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `delete_agent_request` | `agent.delete.request` | agentId | `crates/provider/src/rpc/agent_runtime.rs:45` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `diagnostics.request` | `diagnostics.request` | — | `crates/metadata/src/dispatch.rs:155`<br>`crates/metadata/src/rpc/daemon.rs:75` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `dictation_stream_cancel` | `dictation.stream.cancel` | dictationId | `crates/voice/src/connection.rs:100`<br>`crates/voice/src/connection/dictation.rs:157` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `dictation_stream_chunk` | `dictation.stream.chunk` | dictationId, seq, audio, format | `crates/voice/src/connection.rs:98`<br>`crates/voice/src/connection/dictation.rs:141` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `dictation_stream_finish` | `dictation.stream.finish` | dictationId, finalSeq | `crates/voice/src/connection.rs:99`<br>`crates/voice/src/connection/dictation.rs:149` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `dictation_stream_start` | `dictation.stream.start` | dictationId, format | `crates/voice/src/connection.rs:97`<br>`crates/voice/src/connection/dictation.rs:138` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `directory_suggestions_request` | `directory.suggestions.request` | query, cwd, includeFiles, includeDirectories, matchMode, limit | `crates/filesystem/src/rpc/files.rs:21` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fetch_agent_history_request` | `agent.history.get.request` | filter, search, sort, page | `crates/provider/src/rpc/agent_runtime.rs:41` | 修复 keyset 分页及上游多字段模糊搜索 |
| `fetch_agent_request` | `agent.get.request` | agentId | `crates/provider/src/connection.rs:153`<br>`crates/provider/src/rpc/agent_runtime.rs:42` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fetch_agent_timeline_request` | `agent.timeline.get.request` | agentId, direction, cursor, limit, projection, mergeWindow | `crates/provider/src/rpc/agent_execution.rs:91` | 修复完整时间线投影、分页和连续确认游标 |
| `fetch_agents_request` | `agent.list.request` | scope, filter, sort, page, subscribe, sync | `crates/provider/src/dispatch.rs:132`<br>`crates/provider/src/rpc/agent_execution.rs:63` | 修复 keyset 分页、sync 与连接所有权订阅；同步包含原生权限状态 |
| `fetch_recent_provider_sessions_request` | `provider.sessions.recent.list.request` | cwd, providers, since, limit, query | `crates/provider/src/rpc/agent_execution.rs:76`<br>`crates/provider/src/rpc/agent_execution/native_sessions.rs:21` | 核对输入与路由；仅安装 Codex/Claude，外部账号服务未做真实调用 |
| `fetch_workspaces_request` | `workspace.list.request` | filter, sort, page, subscribe, sync | `crates/metadata/src/dispatch.rs:134`<br>`crates/metadata/src/rpc/directory.rs:58` | 修复 keyset 分页、过滤、状态聚合、sync 与连接所有权订阅 |
| `file_download_token_request` | `fs.file.download_token.request` | cwd, path | `crates/filesystem/src/rpc/files.rs:28` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `file_explorer_request` | `fs.explorer.request` | cwd, path, mode, acceptBinary, maxBytes | `crates/filesystem/src/connection/files/connection.rs:92`<br>`crates/filesystem/src/rpc/files.rs:22` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `file.upload.request` | `file.upload.request` | fileName, mimeType, size, modifiedAt | `crates/api/src/connection.rs:243`<br>`crates/filesystem/src/connection/files/connection.rs:88` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `forge.search.request` | `forge.search.request` | cwd, query, limit, kinds | `crates/filesystem/src/rpc/forge.rs:17` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `fs.entry.create.request` | `fs.entry.create.request` | cwd, parentPath, name, kind | `crates/filesystem/src/rpc/files.rs:24` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fs.entry.delete.request` | `fs.entry.delete.request` | cwd, path | `crates/filesystem/src/rpc/files.rs:27` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fs.entry.duplicate.request` | `fs.entry.duplicate.request` | cwd, path | `crates/filesystem/src/rpc/files.rs:26` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fs.entry.rename.request` | `fs.entry.rename.request` | cwd, path, name | `crates/filesystem/src/rpc/files.rs:25` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fs.file.subscribe.request` | `fs.file.subscribe.request` | cwd, path, subscriptionId | `crates/filesystem/src/connection/files/connection.rs:75` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fs.file.unsubscribe.request` | `fs.file.unsubscribe.request` | subscriptionId | `crates/filesystem/src/connection/files/connection.rs:80` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `fs.file.write.request` | `fs.file.write.request` | cwd, path, content, expectedModifiedAt, expectedRevision | `crates/filesystem/src/rpc/files.rs:23` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `get_daemon_config_request` | `daemon.config.get.request` | — | `crates/metadata/src/connection/daemon.rs:28`<br>`crates/metadata/src/rpc/daemon.rs:82` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `get_providers_snapshot_request` | `provider.snapshot.get.request` | cwd, ifNoneMatch | `crates/provider/src/rpc/agent_execution.rs:85`<br>`crates/provider/src/service/provider_catalog.rs:52` | 修复全局目录作用域及 cwd 规范化 |
| `github_search_request` | `github.search.request` | cwd, query, limit, kinds | `crates/filesystem/src/rpc/forge.rs:18` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `hub.execution.agent.create.request` | 已移除 | executionId, provider, cwd, prompt, workspaceId, model, modeId, thinkingOptionId, featureValues, providerOptions, toolPolicy, env, mcpServers, worktree | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `hub.execution.agent.validate.request` | 已移除 | provider, model, modeId, thinkingOptionId, providerOptions | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `hub.execution.control.request` | 已移除 | executionId, action | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `hub.management.daemon.connect.request` | 已移除 | hubUrl, token, permissions | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `hub.management.daemon.disconnect.request` | 已移除 | force | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `hub.management.daemon.get_status.request` | 已移除 | — | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `hub.management.daemon.permissions.update.request` | 已移除 | grant, revoke | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `import_agent_request` | `agent.import.request` | provider, providerId, sessionId, providerHandleId, cwd, workspaceId, labels | `crates/provider/src/rpc/agent_execution.rs:77`<br>`crates/provider/src/rpc/agent_execution/native_sessions.rs:26` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `kill_terminal_request` | `terminal.kill.request` | terminalId | `crates/terminal/src/rpc.rs:47` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `list_available_editors_request` | `editor.available.list.request` | — | `crates/metadata/src/rpc/editor.rs:31` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `list_available_providers_request` | `provider.available.list.request` | — | `crates/api/src/connection/dispatch.rs:135`<br>`crates/provider/src/rpc/agent_execution.rs:81` | 核对输入与路由；仅安装 Codex/Claude，外部账号服务未做真实调用 |
| `list_commands_request` | `agent.commands.list.request` | agentId, draftConfig | `crates/provider/src/rpc/agent_execution.rs:70`<br>`crates/provider/src/rpc/agent_execution/controls.rs:32` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `list_provider_features_request` | `provider.features.list.request` | draftConfig | `crates/provider/src/rpc/agent_execution.rs:84`<br>`crates/provider/src/service/provider_catalog.rs:44` | 修复 draftConfig 输入和当前草稿的功能值；失败内联返回 |
| `list_provider_models_request` | `provider.models.list.request` | provider, cwd | `crates/provider/src/rpc/agent_execution.rs:82`<br>`crates/provider/src/service/provider_catalog.rs:239` | 修复隐藏模型过滤与全局目录作用域 |
| `list_provider_modes_request` | `provider.modes.list.request` | provider, cwd | `crates/provider/src/rpc/agent_execution.rs:83`<br>`crates/provider/src/service/provider_catalog.rs:240` | 修复全局目录作用域及 cwd 规范化 |
| `list_terminals_request` | `terminal.list.request` | cwd, workspaceId | `crates/terminal/src/rpc.rs:24` | 补齐 hook activity 投影、退出撤销及 Workspace 状态聚合 |
| `loop/inspect` | 已移除 | id | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `loop/list` | 已移除 | — | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `loop/logs` | 已移除 | id, afterSeq | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `loop/run` | 已移除 | prompt, cwd, provider, model, modeId, workerProvider, workerModel, verifierProvider, verifierModel, verifierModeId, verifyPrompt, verifyChecks, archive, name, sleepMs, maxIterations, maxTimeMs | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `loop/stop` | 已移除 | id | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `open_in_editor_request` | `editor.open.request` | path, editorId, mode, cwd | `crates/metadata/src/rpc/editor.rs:35` | 与当前上游一致：提示改由桌面端打开编辑器 |
| `open_project_request` | `workspace.open.request` | cwd | `crates/metadata/src/rpc/directory.rs:54` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `paseo_worktree_archive_request` | `workspace.worktree.archive.request` | worktreePath, repoRoot, branchName, workspaceId, scope, deleteWorktreeFromDisk | `crates/api/src/connection/workspace_archive.rs:17`<br>`crates/filesystem/src/rpc/worktrees.rs:40` | 分阶段归档、关闭资源、复核活动引用后删除；返回真实 removedAgents |
| `paseo_worktree_list_request` | `workspace.worktree.list.request` | cwd, repoRoot | `crates/filesystem/src/rpc/worktrees.rs:38` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `ping` | `connection.ping` | clientSentAt | `crates/metadata/src/dispatch.rs:269` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `plugin.catalog.get.request` | 已移除 | — | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.directory.inspect.request` | 已移除 | path | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.directory.install.request` | 已移除 | path, id | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.disable.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.enable.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.list.request` | 已移除 | — | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.logs.get.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.reload.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.remove.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.rpc.invoke.request` | 已移除 | pluginId, method, input | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.source.install.request` | 已移除 | source, id, ref, pluginPath | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.source.status.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.source.update.apply.request` | 已移除 | proposals | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.source.update.preview.request` | 已移除 | pluginId, target | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `plugin.source.update.request` | 已移除 | pluginId | — | 已按 ADR-045/047/061 移除，不重新接入 |
| `project_icon_request` | `project.icon.get.request` | cwd | `crates/metadata/src/rpc/directory.rs:50` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `project.add.request` | `project.add.request` | cwd | `crates/metadata/src/rpc/directory.rs:45` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `project.create_directory.request` | `project.create_directory.request` | parentPath, name | `crates/metadata/src/rpc/directory.rs:46` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `project.github.clone.request` | `project.github.clone.request` | repo, cloneProtocol, targetDirectory | `crates/filesystem/src/rpc/github_projects.rs:27` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `project.icon.get.request` | `project.icon.get.request` | projectId | `crates/metadata/src/rpc/directory.rs:50` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `project.icon.set.request` | `project.icon.set.request` | projectId, source | `crates/metadata/src/rpc/directory.rs:49` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `project.list.request` | `project.list.request` | sync | `crates/metadata/src/rpc/directory.rs:51` | 修复全量/增量同步及删除序列 |
| `project.remove.request` | `project.remove.request` | projectId | `crates/api/src/connection/workspace_archive.rs:16`<br>`crates/metadata/src/rpc/directory.rs:53` | 修复全部所属 Workspace 的资源清理，支持归档记录重试 |
| `project.rename.request` | `project.rename.request` | projectId, customName | `crates/metadata/src/rpc/directory.rs:52` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `provider_diagnostic_request` | `provider.diagnostic.request` | provider | `crates/provider/src/rpc/agent_execution.rs:74`<br>`crates/provider/src/rpc/agent_execution/controls.rs:21` | 核对输入与路由；仅安装 Codex/Claude，外部账号服务未做真实调用 |
| `provider.usage.list.request` | `provider.usage.list.request` | — | `crates/provider/src/rpc/agent_execution.rs:75`<br>`crates/provider/src/rpc/agent_execution/controls.rs:28` | 核对输入与路由；仅安装 Codex/Claude，外部账号服务未做真实调用 |
| `pull_request_timeline_request` | `checkout.pr.timeline.request` | cwd, prNumber, repoOwner, repoName | `crates/filesystem/src/rpc/forge.rs:22` | 核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入 |
| `push.unregister.request` | `push.unregister.request` | token | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `read_project_config_request` | `project.config.read.request` | repoRoot | `crates/metadata/src/rpc/directory.rs:47` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `refresh_agent_request` | `agent.refresh.request` | agentId | `crates/provider/src/rpc/agent_execution.rs:78`<br>`crates/provider/src/rpc/agent_execution/native_sessions.rs:27` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `refresh_providers_snapshot_request` | `provider.snapshot.refresh.request` | cwd, providers | `crates/provider/src/rpc/agent_execution.rs:86`<br>`crates/provider/src/service/provider_catalog.rs:56` | 修复全局目录作用域及 cwd 规范化 |
| `register_push_token` | `push.register` | token | `crates/api/src/connection.rs:294` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `restart_server_request` | `server.restart.request` | reason | `crates/metadata/src/rpc/daemon.rs:29` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `resume_agent_request` | `agent.resume.request` | handle, overrides | `crates/provider/src/rpc/agent_execution.rs:95` | 修复 overrides、显式归档恢复、未知原生 handle 直接恢复及 cwd 覆盖后继续输入 |
| `schedule/create` | `schedule.create.request` | prompt, name, cadence, target, maxRuns, expiresAt, runOnCreate | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/delete` | `schedule.delete.request` | scheduleId | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/inspect` | `schedule.inspect.request` | scheduleId | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/list` | `schedule.list.request` | — | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/logs` | `schedule.logs.request` | scheduleId | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/pause` | `schedule.pause.request` | scheduleId | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/resume` | `schedule.resume.request` | scheduleId | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/run-once` | `schedule.run_once.request` | scheduleId | `crates/schedule/src/dispatch.rs:31`<br>`crates/schedule/src/service.rs:197` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `schedule/update` | `schedule.update.request` | scheduleId, name, prompt, cadence, newAgentConfig, maxRuns, expiresAt | 见对应 capability 分派 | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `send_agent_message_request` | `agent.message.send.request` | agentId, text, messageId, activeTurnBehavior, images, attachments | `crates/provider/src/rpc/agent_execution.rs:96` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `session.events.set_subscription.request` | `session.events.set_subscription.request` | events, notifications | 见对应 capability 分派 | 补齐 terminal attention、审批及原生子 Agent 事件；checkout、script/setup 等事件类别未全部实现 |
| `set_agent_feature_request` | `agent.feature.set.request` | agentId, featureId, value | `crates/provider/src/rpc/agent_execution.rs:101`<br>`crates/provider/src/rpc/agent_execution/controls.rs:72` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `set_agent_mode_request` | `agent.mode.set.request` | agentId, modeId | `crates/provider/src/rpc/agent_execution.rs:100`<br>`crates/provider/src/rpc/agent_execution/controls.rs:71` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `set_agent_model_request` | `agent.model.set.request` | agentId, modelId | `crates/provider/src/rpc/agent_execution.rs:97`<br>`crates/provider/src/rpc/agent_execution/controls.rs:69` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `set_agent_thinking_request` | `agent.thinking.set.request` | agentId, thinkingOptionId | `crates/provider/src/rpc/agent_execution.rs:98`<br>`crates/provider/src/rpc/agent_execution/controls.rs:70` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `set_daemon_config_request` | `daemon.config.set.request` | config | `crates/metadata/src/connection/daemon.rs:21`<br>`crates/metadata/src/rpc/daemon.rs:88` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `set_voice_mode` | `voice.mode.set.request` | enabled, agentId | `crates/voice/src/connection/voice.rs:130`<br>`crates/voice/src/dispatch.rs:29` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `shutdown_server_request` | `server.shutdown.request` | — | `crates/metadata/src/rpc/daemon.rs:45` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `start_workspace_script_request` | `workspace.script.start.request` | workspaceId, scriptName | `crates/metadata/src/rpc/workspace_automation.rs:33` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `stash_list_request` | `checkout.stash.list.request` | cwd, paseoOnly | `crates/filesystem/src/rpc/checkout.rs:32` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `stash_pop_request` | `checkout.stash.pop.request` | cwd, stashIndex | `crates/filesystem/src/rpc/checkout.rs:31` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `stash_save_request` | `checkout.stash.save.request` | cwd, branch | `crates/filesystem/src/rpc/checkout.rs:30` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `subscribe_checkout_diff_request` | `checkout.diff.subscribe.request` | subscriptionId, cwd, compare | `crates/filesystem/src/connection/checkout.rs:98`<br>`crates/filesystem/src/dispatch.rs:179` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `subscribe_terminal_request` | `terminal.subscribe.request` | terminalId, restore | `crates/terminal/src/connection.rs:84` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `subscribe_terminals_request` | `terminal.list.subscribe.request` | cwd, workspaceId | `crates/terminal/src/connection.rs:89` | 活动变化进入目录事件；终端归属按 Workspace 身份隔离 |
| `subscription.release.request` | `subscription.release.request` | subscriptionId | `crates/metadata/src/dispatch.rs:249` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `terminal_input` | `terminal.input` | terminalId, message | `crates/api/src/connection.rs:226` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `terminal.rename.request` | `terminal.rename.request` | terminalId, title | `crates/terminal/src/rpc.rs:40` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `unsubscribe_checkout_diff_request` | `checkout.diff.unsubscribe.request` | subscriptionId | `crates/filesystem/src/dispatch.rs:166` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `unsubscribe_terminal_request` | `terminal.unsubscribe.request` | terminalId | `crates/terminal/src/connection.rs:94` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `unsubscribe_terminals_request` | `terminal.list.unsubscribe.request` | cwd, workspaceId | `crates/terminal/src/connection.rs:101` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `update_agent_request` | `agent.update.request` | agentId, name, labels | `crates/provider/src/rpc/agent_runtime.rs:43` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `validate_branch_request` | `checkout.branch.validate.request` | cwd, branchName | `crates/filesystem/src/rpc/checkout.rs:20` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `voice_audio_chunk` | `voice.audio.chunk` | audio, format, isLast | `crates/voice/src/connection.rs:103` | 核对输入与路由；使用 Ait 本地语音后端，见 ADR-064 |
| `wait_for_finish_request` | `agent.finish.wait.request` | agentId, timeoutMs | `crates/provider/src/dispatch.rs:158`<br>`crates/provider/src/dispatch/agent_execution.rs:43` | 修复无限/长超时、审批、未知身份内联错误及自动归档期间保留最后回复 |
| `workspace_setup_status_request` | `workspace.setup.status.request` | workspaceId | `crates/metadata/src/rpc/workspace_automation.rs:30` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.clear_attention.request` | `workspace.clear_attention.request` | workspaceId | `crates/metadata/src/rpc/workspace_state.rs:33` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.create.request` | `workspace.create.request` | workspaceId, agent, subscribe, idempotencyKey, title, firstAgentContext, source | `crates/api/src/connection/workspace_creation.rs:15`<br>`crates/metadata/src/dispatch.rs:129` | 补齐初始 Agent、共享回执、断线继续、并发去重、安全重试、身份冲突与完成回执验证；GitHub PR checkout 可用 |
| `workspace.github.search_repositories.request` | `workspace.github.search_repositories.request` | query, limit | `crates/filesystem/src/rpc/github_projects.rs:28` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.label.assignment.set.request` | `workspace.label.assignment.set.request` | workspaceId, label, assigned | `crates/metadata/src/rpc/workspace_labels.rs:123` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.label.delete.inspect.request` | `workspace.label.delete.inspect.request` | name | `crates/metadata/src/rpc/workspace_labels.rs:125` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.label.delete.request` | `workspace.label.delete.request` | name | `crates/metadata/src/rpc/workspace_labels.rs:126` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.label.list.request` | `workspace.label.list.request` | subscribe, sync | `crates/metadata/src/dispatch.rs:208`<br>`crates/metadata/src/rpc/workspace_labels.rs:122` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.label.update.request` | `workspace.label.update.request` | name, newName, color | `crates/metadata/src/rpc/workspace_labels.rs:124` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.mark_unread.request` | `workspace.mark_unread.request` | workspaceId | `crates/metadata/src/rpc/workspace_state.rs:34` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.pin.set.request` | `workspace.pin.set.request` | workspaceId, pinned | `crates/metadata/src/rpc/directory.rs:61` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.recovery.inspect.request` | `workspace.recovery.inspect.request` | workspaceId | `crates/filesystem/src/rpc/workspace_recovery.rs:33` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.recovery.restore.request` | `workspace.recovery.restore.request` | workspaceId | `crates/filesystem/src/rpc/workspace_recovery.rs:34` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.script.list.request` | `workspace.script.list.request` | workspaceId | `crates/metadata/src/rpc/workspace_automation.rs:32` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.script.start.request` | `workspace.script.start.request` | workspaceId, scriptName | `crates/metadata/src/rpc/workspace_automation.rs:33` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.script.stop.request` | `workspace.script.stop.request` | workspaceId, scriptName | `crates/metadata/src/rpc/workspace_automation.rs:34` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.setup.run.request` | `workspace.setup.run.request` | workspaceId | `crates/metadata/src/rpc/workspace_automation.rs:31` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `workspace.title.set.request` | `workspace.title.set.request` | workspaceId, title | `crates/metadata/src/rpc/directory.rs:60` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
| `write_project_config_request` | `project.config.write.request` | repoRoot, config, expectedRevision | `crates/metadata/src/rpc/directory.rs:48` | 名称、输入字段与实现入口已核对；语义证据见关联测试及主报告 |
