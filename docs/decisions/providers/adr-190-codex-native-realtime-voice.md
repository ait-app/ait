# ADR-190：Codex Provider 拥有原生实时语音

- 状态：Accepted
- 日期：2026-10-25
- 修订：[ADR-042](../daemon/adr-042-daemon-voice.md)、[ADR-064](../daemon/adr-064-offline-speech.md) 对 Codex 语音对话的实现选择；听写保持原有后端，旧客户端的通用语音协议继续兼容。

## 背景

空对话只开放前端入口，仍会执行 Ait 的 STT → Agent 文本轮次 → TTS，不能提供 Codex 原生实时语音。
Codex app-server 已提供 `thread/realtime/*`，其音频、认证、原生工具轮次与生命周期应由 Codex adapter 管理。

## 决策

- Codex adapter 声明 `supportsRealtimeVoice`，通过 Provider 目录和 Agent capability 快照发布。客户端统一按该能力决定草稿和已有 Agent 的语音入口及快捷键是否可用；能力未声明或为 false 时不提供入口，不推断 Provider 名称。创建回调和消息是否为空均不是能力来源。
- 音频和文本复用同一个 Codex thread 与 app-server transport。独立音频事件在前台 turn 过滤前处理；原生产生的工具轮次与审批继续使用既有 provider 管线。
- `model::realtime` 定义有界音频队列、SDP 与生命周期事件。`provider` 拥有 native endpoint；`voice` 拥有连接侧 session；daemon 仅桥接端口，两项能力互不依赖。
- 桌面/Web 浏览器录音与播放使用 WebRTC，SDP 由 Codex `thread/realtime/start` / `thread/realtime/sdp` 协商。显式使用 realtime v3；Codex 0.163.0 的默认 WebRTC v1 会被 AVAS 拒绝。认证始终留在 Codex。
- 无 WebRTC 引擎的客户端使用 Codex 原生 WebSocket PCM 通道。Codex 当前要求此通道具有 API key 认证。Provider 将输入连续重采样为 24 kHz，并验证输出格式、大小与通道数。
- Native 启动错误直接返回，不回退到 Ait STT/TTS。Native 不依赖离线语音模型准备状态。未声明能力的 Provider 不显示实时语音入口。旧客户端请求的通用语音协议及听写保持兼容。
- Session drop、显式停止与连接断开都会取消对应原生会话。停止确认前按原顺序保留普通消息、审批和 canonical speech 事件，仅排空旧音频、SDP 和传输关闭通知。已结束会话的记录仍进入 Timeline，但不能改变新音频会话的录音或播放状态。
- 队列满时结束原生音频，不无限缓存。PCM 播放仍有四片确认窗口与超时；WebRTC 的媒体传输由浏览器和 Codex 协商的服务端负责。
- Codex adapter 将完整 `realtime_delegation` 包装解析为用户消息上的可选 `voiceTranscript` 展示字段，按 `user` / `assistant` 保留语音片段顺序与换行；截断后角色不明的开头保持为 `unknown`，不猜测发言者。解析限制 64 KiB、256 段，普通文本、引用示例和无法识别的格式保持原样。实时完成与原生历史刷新使用相同投影，保留原始消息和轮次身份，不改写 Codex 的上下文。
- 客户端按通用语音片段渲染用户气泡和助手正文，并在消息流与本地缓存保留这些片段。片段附属于一次交接，不伪造额外的文本轮次、消息 ID 或回退操作；已有转写时不重复渲染 `input`。
- Canonical 语音通过 `thread/realtime/item/started`、`item/transcript/delta`、`item/completed` 投影为通用 `voice` Timeline 条目。条目包含原生片段 ID、语音会话 ID、双方角色、完整文本快照或开始/结束边界；无普通文本 turn 身份，不进入消息提交、审批和回退管线。流式快照按同一身份替换，完成文本允许修正转写，不要求它以增量文本为前缀。该协议扩展由配套客户端、SDK 与 daemon 一起发布。
- 原生历史通过有界分页的 `thread/timeline/list` 读取，按 `position` 保留普通消息和语音的原始顺序。只在明确的 method-not-found（-32601）或精确匹配该方法名的 unknown-variant（-32600）时回退 `thread/read`，其他错误保持可见。实时与历史共用语音投影；已被 canonical 语音覆盖的交接片段不重复展示，独立输入和未覆盖片段保留。
- `bemItemPromoted` 按 `turnId` / `itemId` 关联普通源消息。已展示的源消息不再插入；缺少源消息时用 `thread/items/read` 只读补齐，复用既有 Markdown、工具与可视化渲染。`wholeItem`、`inlineMarkdown`、`inlineVisualization` 当前均展示完整源条目，不另建重复副本，也不重放工具交互。
- 语音文本进入 Provider 的持久化 Timeline 和客户端缓存，不通过仅面向录音连接的音频队列广播。单片段限制 64 KiB，实时状态限制 8192 项、16 MiB 文本，原生历史限制 32768 条、32 MiB 和 512 个分页游标。

## 后果

空 Agent 通过独立创建操作启动，仍校验 Provider、模型、目录和连接；普通消息提交需要正文或附件，两者不通过 `allowEmptyText` 开关混用，也不伪造首条消息。Provider 负责能力与认证差异，客户端只负责通用的媒体设备和 SDP。
SDP 请求字段和能力声明保持可选，没有 WebRTC 引擎的配套客户端仍可使用原生 PCM；新增 canonical voice 条目需要配套客户端支持，原生协议升级需更新 Codex adapter。
`appendText`、`appendSpeech`、音色选择与手动 handoff 不属于本次时间线扩展。`flushTranscriptTailOnSessionEnd` 保持原生默认值，挂断不额外触发 Codex 文本任务；最终语音记录的持久化由 canonical Timeline 完成。
移动端当前没有 WebRTC 引擎，使用 PCM 原生通道时仍受 Codex API key 要求限制，不能将桌面/Web 的登录兼容性等同于移动端。

协议依据为安装的 Codex 0.163.0 `app-server generate-ts --experimental`，以及 [Codex 原生实时会话实现](https://github.com/openai/codex/blob/main/codex-rs/core/src/realtime_conversation.rs)。
语音交接包装与转义规则依据 [Codex RealtimeDelegation](https://github.com/openai/codex/blob/main/codex-rs/core/src/context/realtime_delegation.rs)。
