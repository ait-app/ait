# ADR-191：Codex Provider 拥有原生实时语音

- 状态：Accepted
- 日期：2026-10-25
- 修订：[ADR-042](../daemon/adr-042-daemon-voice.md)、[ADR-064](../daemon/adr-064-offline-speech.md) 对 Codex 语音对话的实现选择；听写保持原有后端，旧客户端的通用语音协议继续兼容。

## 背景

空对话只开放前端入口，仍会执行 Ait 的 STT → Agent 文本轮次 → TTS，不能提供 Codex 原生实时语音。
Codex app-server 已提供 `thread/realtime/*`，其音频、认证、原生工具轮次与生命周期应由 Codex adapter 管理。

## 决策

- Codex adapter 声明 `supportsRealtimeVoice`，通过 Provider 目录和 Agent capability 快照发布。客户端统一按该能力决定草稿和已有 Agent 的语音入口及快捷键是否可用；能力未声明或为 false 时不提供入口，不推断 Provider 名称。创建回调和消息是否为空均不是能力来源。
- 音频会话和文本复用同一个 Codex thread，信令及工具事件复用 app-server transport。实时会话事件在前台 turn 过滤前处理；原生产生的工具轮次与审批继续使用既有 provider 管线。
- `model::realtime` 定义有界 PCM 音频队列、媒体就绪信号与生命周期事件。`provider` 拥有 native endpoint；`voice` 拥有连接侧 session；daemon 仅桥接端口，两项能力互不依赖。
- 所有客户端只与 daemon 交互：麦克风通过 `voice.audio.chunk` 上传单声道 PCM16，播放通过 `voice.audio.output` 与确认窗口下发。直连与 online relay 使用相同协议；客户端不持有 OpenAI SDP、地址或认证，也不创建上游 WebRTC peer。
- 客户端先完成原生语音所需的麦克风授权与采集初始化，再请求 daemon 启动媒体会话。等待授权期间不启动上游音频，避免尚未进入语音状态时忽略首批 PCM 和播放确认而填满队列；daemon 就绪前不上传采集数据。授权失败或启动期间断线均释放本地设备。
- daemon 内的 Codex adapter 拥有 WebRTC peer、ICE/DTLS/SRTP、Opus 编解码与连续 24 kHz 重采样。offer 经已有 Codex app-server 的 `thread/realtime/start` 发送，`thread/realtime/sdp` 的 answer 仅在 provider 内使用。显式使用 realtime v3，保留 Codex 登录认证，不改走要求 API key 的原生 WebSocket 音频入口。
- 只有上游媒体连接就绪后才接受语音启动；连接超时、启动被放弃、媒体错误与原生关闭均取消 peer 和对应会话。输入按 20 ms 编码，输出按约 80 ms 合并 PCM，乱序、丢包补偿与所有音频缓存均有界。
- 转写完成不是音频播放边界：助手 RTP 可以先于用户 `transcript/done` 到达。插话只一次性清理已缓冲的旧播放，保留连续 Opus 解码状态；后续 RTP 正常解码下发，不等待转写完成、不用转写状态持续静音。转写完成也不重置解码器。
- online relay 透明中继控制、PCM、播放确认和关闭事件；不新增上游地址配置或旁路媒体连接。relay 断开使物理 daemon 连接关闭，并沿 Session 生命周期释放上游媒体。
- Native 启动错误直接返回，不回退到 Ait STT/TTS。Native 不依赖离线语音模型准备状态。未声明能力的 Provider 不显示实时语音入口。旧客户端请求的通用语音协议及听写保持兼容。
- Session drop、显式停止与连接断开都会取消对应原生会话。停止确认前按原顺序保留普通消息、审批和 canonical speech 事件，仅排空旧音频、SDP 和传输关闭通知。已结束会话的记录仍进入 Timeline，但不能改变新音频会话的录音或播放状态。
- 队列满时结束原生音频，不无限缓存。小块原生 PCM 播放采用 16 片确认窗口以覆盖 relay 往返延迟，通用语音保持四片；两者均有超时。WebRTC 媒体传输由 daemon provider 和 Codex 协商的服务端负责。
- 播放积压不能阻塞控制事件：连接侧仍按有界批次读取原生事件，插话丢弃旧播放后保留同批后续回复，关闭与失败立即释放目标。没有插话消除的音频溢出立即取消会话，不等待播放确认超时。
- iOS/Android 连续 PCM 提前进入原生播放队列。iOS 使用 `dataPlayedBack`，Android 使用 `AudioTrack` 消费帧位置确认完成，不能用 JS 时长定时器或写入完成替代播放完成；原生队列限制 32 片、1 MiB，停止清空队列及帧计数。JS 的授权、初始化和解码等待均检查会话代次，取消后的结果不得启动录音、排入旧音频或确认新会话。新增原生完成接口要求配套移动端二进制，旧二进制明确拒绝播放，不伪造确认。
- 原生下行 PCM 是按序的连续音频，客户端收到后立即排入设备播放队列，不以每片的结束回调驱动下一片。Web/Electron 在同一音频时钟上提前调度，首次播放预留 120 ms，欠载后仅预留 20 ms 恢复裕量，避免每次输入停顿都累加完整启动延迟；24 kHz 输出图连续转换到设备采样率，避免各短片分别重采样的边界尖峰。连续超过 240 ms 的全零 PCM 在排队超过 240 ms 时可直接消费，压缩传输停顿后的过时静音；任何非零采样均完整播放。语音片在实际播完后确认，压缩的全零片按消费确认；插话、关闭和重连取消全部已调度与待解码音频，旧回调不得确认新会话。
- Codex adapter 将完整 `realtime_delegation` 包装解析为用户消息上的可选 `voiceTranscript` 展示字段，按 `user` / `assistant` 保留语音片段顺序与换行；截断后角色不明的开头保持为 `unknown`，不猜测发言者。解析限制 64 KiB、256 段，普通文本、引用示例和无法识别的格式保持原样。实时完成与原生历史刷新使用相同投影，保留原始消息和轮次身份，不改写 Codex 的上下文。
- 客户端按通用语音片段渲染用户气泡和助手正文，并在消息流与本地缓存保留这些片段。片段附属于一次交接，不伪造额外的文本轮次、消息 ID 或回退操作；已有转写时不重复渲染 `input`。
- Canonical 语音通过 `thread/realtime/item/started`、`item/transcript/delta`、`item/completed` 投影为通用 `voice` Timeline 条目。条目包含原生片段 ID、语音会话 ID、双方角色、完整文本快照或开始/结束边界；无普通文本 turn 身份，不进入消息提交、审批和回退管线。流式快照按同一身份替换，完成文本允许修正转写，不要求它以增量文本为前缀。该协议扩展由配套客户端、SDK 与 daemon 一起发布。
- 原生历史通过有界分页的 `thread/timeline/list` 读取，按 `position` 保留普通消息和语音的原始顺序。只在明确的 method-not-found（-32601）或精确匹配该方法名的 unknown-variant（-32600）时回退 `thread/read`，其他错误保持可见。实时与历史共用语音投影；已被 canonical 语音覆盖的交接片段不重复展示，独立输入和未覆盖片段保留。
- `bemItemPromoted` 按 `turnId` / `itemId` 关联普通源消息。已展示的源消息不再插入；缺少源消息时用 `thread/items/read` 只读补齐，复用既有 Markdown、工具与可视化渲染。`wholeItem`、`inlineMarkdown`、`inlineVisualization` 当前均展示完整源条目，不另建重复副本，也不重放工具交互。
- 语音文本进入 Provider 的持久化 Timeline 和客户端缓存，不通过仅面向录音连接的音频队列广播。单片段限制 64 KiB，实时状态限制 8192 项、16 MiB 文本。原生历史限制由 [ADR-192](adr-192-codex-incremental-history-projection.md) 修订为 32768 条、32 MiB 展示投影和 512 页，图片及工具结果在计入展示预算前按页转换。

## 后果

空 Agent 通过独立创建操作启动，仍校验 Provider、模型、目录和连接；普通消息提交需要正文或附件，两者不通过 `allowEmptyText` 开关混用，也不伪造首条消息。Provider 负责能力与认证差异，客户端只负责通用媒体设备与 daemon 的 PCM 协议。
移除客户端协议中的 SDP 字段；前期直连 WebRTC 测试客户端需与 daemon 一起更新。移动端也复用 provider 的 WebRTC 认证，不再受原生 WebSocket 音频入口的 API key 限制。新增 canonical voice 条目需要配套客户端支持，原生协议升级需更新 Codex adapter。
`appendText`、`appendSpeech`、音色选择与手动 handoff 不属于本次时间线扩展。`flushTranscriptTailOnSessionEnd` 保持原生默认值，挂断不额外触发 Codex 文本任务；最终语音记录的持久化由 canonical Timeline 完成。
构建 daemon 需要 CMake 与 C/C++ 工具链；Opus 由 bundled libopus 静态链接，无运行时系统库要求。daemon 所在网络须能访问 Codex 实时服务并建立媒体连接；online relay 只解决客户端至 daemon 的可达性。

协议依据为安装的 Codex 0.163.0 `app-server generate-ts --experimental`，以及 [Codex 原生实时会话实现](https://github.com/openai/codex/blob/main/codex-rs/core/src/realtime_conversation.rs)。
语音交接包装与转义规则依据 [Codex RealtimeDelegation](https://github.com/openai/codex/blob/main/codex-rs/core/src/context/realtime_delegation.rs)。
