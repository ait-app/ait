---
id: ADR-239
status: proposed
date: 2026-11-11
amends:
  - providers/adr-091-independent-session-execution.md
  - workspace/adr-104-filesystem-model-collaboration.md
  - clients/adr-049-app-rust-browser-transport.md
paths:
  - crates/provider
  - crates/model/src/download.rs
  - crates/filesystem/src/files
  - crates/api/src/connection
  - crates/api/src/guest
  - crates/api/src/lib.rs
  - crates/api/src/browser_auth.rs
  - packages/protocol
  - packages/client
  - apps/mobile/src/components
  - apps/mobile/src/stores/download-store.ts
  - apps/mobile/src/runtime/daemon-transport
  - apps/desktop/src/daemon
---

# ADR-239：已保存 Timeline 导出与一次性客户端下载

## 背景

通用会话导出需要读取 Ait 已提交的展示记录，而不是调用 Provider 的原生导出命令、读取当前 UI
分页，或把服务器本地路径当作客户端已经保存的文件。导出不能向正在运行的会话发送输入、恢复
writer 或中断轮次。消息与持久结果卡应当可读，不能把每个正常条目都替换为 JSON dump。

本提案属于 PR #612 拆分 task26，依赖 task04 的上下文详情入口；任务和实测证据见
[task26 记录](../../plans/pr612-split/task26.json)，总契约见 [Draft #647](https://github.com/ait-app/ait/pull/647)。
提案状态不代表已经合并或全部验收通过。

## 接口变化与消费者迁移

这些是明确的接口变化，不以“旧字段没变”代替披露；不增加计划之外的公共破坏性变更。

| 边界与所有者                 | 旧接口                                                         | 新接口                                                                                                                        | 迁移、兼容与未改变项                                                                                                                                                                    |
| ---------------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider/API 公共 RPC        | 没有 Ait 持久 Timeline 导出操作                                | `agent.timeline.export.request` 与 `agent.timeline.export.cancel.request`；格式仅 Markdown/HTML，范围为 conversation/timeline | 新增接口，不替换既有 Timeline、消息或原生接口。没有任意 native-action 网关。                                                                                                            |
| Protocol/SDK                 | 没有导出结果或调用方法                                         | 类型化选项、一次性 artifact receipt；`exportAgentTimeline`、`cancelAgentTimelineExport`                                       | `agentTimelineExport` 必须同时协商两个方法；旧 Host 不发请求，旧客户端继续读取原有消息。生成请求不自动重试。                                                                            |
| Provider → API → Files       | 既有跨服务 Completion 与普通文件 grant                         | 新 Completion 变体交接拥有匿名文件句柄的 `model::download::GeneratedArtifact`，API 交给既有 Files registry                    | API 的穷尽 match 已迁移。Provider 不依赖 filesystem/API；Files 不导入 Provider。没有第二套 token registry。                                                                             |
| 公共 HTTP/browser auth       | 文件 GET 使用一次性 grant；ADR-049 仅换票端点提供 CORS         | 既有 `/api/files/download` 为经过现有 Host/Origin 校验的 GET 和 OPTIONS 返回精确 Origin CORS                                  | 这是 ADR-049 的限定扩展：只允许 GET 与 Authorization 请求头，不使用通配 Origin、cookies 或 credentials 放宽。OPTIONS 不消费 grant；被拒绝的 Origin 也不消费。其他路由 Bearer 规则不变。 |
| Desktop IPC/原生与浏览器传输 | 既有 account download prepare/transfer/cancel 和 relay helpers | 可选 direct target、`maxBytes`、`expectedSize`，以及传输取消信号                                                              | Timeline 调用者提供约束，普通文件调用者保持原有默认行为。完成必须满足 receipt 字节数，不能只相信另一份 HTTP/relay 长度。                                                                |
| 客户端 UI                    | 上下文详情只展示事实                                           | 在同一入口选择格式和已保存内容范围并下载；九种语言提供说明                                                                    | 不增加 Todo、Goal、子 Agent 或会话内容编辑器。浏览器使用 `browser_handoff`，不把 Blob 链接点击冒充磁盘保存成功。                                                                        |

HTTP 文件 grant、visitor binding、既有无效/过期/已消费 token 的 403 行为保持不变；文件 token
仍须由已授权的 RPC 获得。新增 CORS 不让未授权页面读取文件，也不让浏览器伪造 relay visitor
请求头。旧 Host 缺少能力时只显示不可用，不借用原生 OMP HTML 导出或其他原生命令作为回退。

## Provider：只读内容与可读渲染

1. 复用现有可读 Agent 身份和 canonical Timeline 存储，在同一个 SQLite 读事务内按 seq 分页读取
   所有已提交 entries/progress，再复用既有投影。不是当前 UI 页，不触发原生刷新，不创建派生索引、
   不写 Timeline，也不启动、恢复、输入或取消原生会话。
2. `timeline` 包含全部已保存展示条目；`conversation` 仅选择用户/助手消息和语音转写。二者都是
   同一完整持久快照上的选择，不包含未提交编辑器、登录配置或账号凭据存储。
3. 按既有类型呈现消息角色、时间、正文、Todo、错误、通知、压缩、语音、工具与插件结果卡。
   保留实际 main 已有的 host/result card 边界及字段，不引入 donor 独有的 `providerCommand` 控制模型。
   未知持久类型可用结构化 fallback；正常消息不能退化成统一 JSON dump。
4. 附件只列出明确引用，不自动抓取、内嵌或执行外部资源。HTML 转义正文与字段，带禁用外部资源/
   active content 的 CSP；Markdown 正文保留用户原始 Markdown，不声称清洗用户内容中的秘密。
5. 来源最多 20,000 行、16 MiB，输出最多 16 MiB；超过任一限制明确失败，不能返回截断的“完整”文件。
   两个生成槽限制并发；写入匿名临时文件而不是在内存积累整个输出。receipt 的 `entries` 是所选
   投影条目数量，`source` 固定为 `persisted_timeline`。

## API/Files：授权、所有权与回收

- API 使用类型化 Completion 协调两项能力。`GeneratedArtifact` 只承载已经完成的匿名文件、名称、
  MIME、长度及所属 Agent，不对客户端暴露服务器文件路径。
- 复用 Files 的一次性下载 registry、现有 HTTP/Hub/relay 读者和 visitor binding。生成 artifact 最多
  八个槽，已消费但仍在传输的 reader 继续持有槽；一般下载 registry 的既有边界也继续生效。
- grant 有效期 60 秒。注册 grant 与安装到期清理任务在同一个已准入阻塞操作内完成，响应接收者
  消失不能留下无人清理的文件。消费、显式释放、到期、传输终止或关闭都通过所有权释放文件。
- Guest 先校验 Agent 资源范围，并沿用下载额度与 visitor 绑定；取消只释放同一 Agent 的对应生成
  artifact。取消清理不等待普通前台工作额度，不创建新的会话 writer。
- CORS 只在现有来源校验之后添加。预检不访问 Files、不读取或消费 token；真实 GET 仍由原来的
  一次性 grant/visitor 校验决定是否能返回字节。没有为解决浏览器问题放宽认证或增加通用网关。

## UI/平台传输：完成与失败

- UI 复用现有 context details/sheet 和下载状态。连接 epoch/client generation 变化后，迟到 receipt
  不开始当前 Host 的下载；尽力释放原 Host 的 grant，不能把旧连接结果应用到新连接。
- Desktop 继续通过现有准备/下载/取消 IPC，使用主进程保存对话框；HTTP 或 relay 完整接收、校验、
  同步并 rename 后才完成。失败只清理本次 partial，不删除已有目的文件。
- 原生客户端复用现有文件/share 管线，写 partial 后校验并移动；失败或取消只删除本次实际创建或
  移入的文件，不因为路径曾被预留就删除别人的文件。
- 浏览器验证完整流和 receipt 大小后才交给下载器，使用 `browser_handoff` 提醒用户查看浏览器下载。
  应用不能从 anchor 点击确认浏览器最后的磁盘保存；实际保存验收由浏览器 download completion 和
  保存文件字节验证。常规 frame buffer 保持视图，不额外复制每个 chunk。
- 所有传输路径都限制字节数并支持取消。artifact 丢失、过期、长度不符或传输失败时报告失败并回收，
  不重新执行导出、不自动生成新 token、不重放原生输入。

## 验证与限制

定向测试覆盖 canonical 多页读取、消息/真实主分支结果卡、范围选择、转义、边界、单次授权、
来源拒绝、预检与消费、visitor binding、到期清理和传输/文件所有权。真实浏览器验收必须在活动
原生轮次中通过现有可见入口保存 MD/HTML，并证明没有额外输入、writer 或中断。

实测结果与未完成项持续写入 task26 记录，不把原生手机消费者 mock、Node 文件测试或资源接线
描述为实际设备/签名应用验收。此特性不包括原生 OMP HTML 导出、DSH ZIP 导出或新的平台下载系统。
