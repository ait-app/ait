# ADR-190：Codex Provider 拥有原生实时语音

- 状态：Accepted
- 日期：2026-10-25
- 修订：[ADR-042](../daemon/adr-042-daemon-voice.md)、[ADR-064](../daemon/adr-064-offline-speech.md) 对 Codex 语音对话的实现选择；其他 Provider 与听写保持原有后端。

## 背景

空对话只开放前端入口，仍会执行 Ait 的 STT → Agent 文本轮次 → TTS，不能提供 Codex 原生实时语音。
Codex app-server 已提供 `thread/realtime/*`，其音频、认证、原生工具轮次与生命周期应由 Codex adapter 管理。

## 决策

- Codex adapter 声明 `supportsRealtimeVoice`，通过 Provider 目录和 Agent capability 快照发布。草稿界面仅为声明该能力的 Provider 提供空 Agent 语音启动，不推断 Provider 名称。
- 音频和文本复用同一个 Codex thread 与 app-server transport。独立音频事件在前台 turn 过滤前处理；原生产生的工具轮次与审批继续使用既有 provider 管线。
- `model::realtime` 定义有界音频队列、SDP 与生命周期事件。`provider` 拥有 native endpoint；`voice` 拥有连接侧 session；daemon 仅桥接端口，两项能力互不依赖。
- 桌面/Web 浏览器录音与播放使用 WebRTC，SDP 由 Codex `thread/realtime/start` / `thread/realtime/sdp` 协商。显式使用 realtime v3；Codex 0.163.0 的默认 WebRTC v1 会被 AVAS 拒绝。认证始终留在 Codex。
- 无 WebRTC 引擎的客户端使用 Codex 原生 WebSocket PCM 通道。Codex 当前要求此通道具有 API key 认证。Provider 将输入连续重采样为 24 kHz，并验证输出格式、大小与通道数。
- Native 启动错误直接返回，不回退到 Ait STT/TTS。Native 不依赖离线语音模型准备状态。其他 Provider 保持通用语音对话，听写保持原有逻辑。
- Session drop、显式停止与连接断开都会取消对应原生会话。停止确认前保留普通消息与审批事件，并排空上一代 realtime 事件，避免重新启动时受到旧关闭通知影响。
- 队列满时结束原生音频，不无限缓存。PCM 播放仍有四片确认窗口与超时；WebRTC 的媒体传输由浏览器和 Codex 协商的服务端负责。

## 后果

空 Agent 不需要伪造首条消息。Provider 负责能力与认证差异，客户端只负责通用的媒体设备和 SDP。
新请求字段和能力字段均可选，旧客户端仍可使用原有 PCM 方式；原生协议升级需更新 Codex adapter。
移动端当前没有 WebRTC 引擎，使用 PCM 原生通道时仍受 Codex API key 要求限制，不能将桌面/Web 的登录兼容性等同于移动端。

协议依据为安装的 Codex 0.163.0 `app-server generate-ts --experimental`，以及 [Codex 原生实时会话实现](https://github.com/openai/codex/blob/main/codex-rs/core/src/realtime_conversation.rs)。
