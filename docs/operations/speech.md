# 独立 daemon 的语音与听写

听写和非 Codex Provider 的语音对话默认使用自动下载模型的离线 Sherpa ONNX 后端。首次启动后台准备模型，
不阻塞 Host 连接。准备期间点击语音会返回可重试的 `speech_models_preparing`；下载失败返回
`speech_model_download_failed`，稍后再次点击会重试。显式禁用时返回 `speech_backend_unavailable`。

## Codex 原生实时语音

选择 Codex 时可从空对话直接开始语音。服务端创建空 Agent，再通过同一个 Codex thread 启动原生 realtime；不提交空消息，也不调用 Ait 的 STT/TTS。

桌面和 Web 使用 WebRTC。客户端生成 offer，通过 `voice.mode.set.request` 的可选 `sdp` 字段交给 Codex provider；成功响应包含 `sdp` answer。Codex 使用 realtime v3 完成认证、会话协商和原生工具轮次，音频直接通过 WebRTC 传输。浏览器需要允许麦克风访问，Web 需要 HTTPS 或 localhost。

没有 WebRTC 引擎的客户端使用原生 WebSocket PCM。Codex 0.163.0 要求该通道使用 API key 认证；ChatGPT 登录在桌面/Web 通过 WebRTC 使用。原生版本或认证不支持时返回 `native_realtime_failed`，不会改用通用语音后端。离线模型是否下载完成不影响 Codex 原生语音。

关闭语音、断线或原生会话结束会释放媒体设备和连接。`voice.session.closed` 通知客户端停止录音和播放；native 转写只是显示事件，不再转成 Ait 文本消息。

## 后端配置

默认无需配置，也不需要安装 whisper-cli、Piper 或 Python。识别使用 SenseVoice int8（中、英、日、韩、粤），
合成使用 Kokoro multi-lang v1_0 的中文 voice 45。模型来自 Sherpa 官方 GitHub release，缓存于
`<server-data-dir>/models/local-speech`，安装后约 637 MiB；首次准备需要联网，之后识别和合成可离线运行。
语音对话仍需所选 Agent Provider 正常工作。

```sh
export AIT_SPEECH_PROVIDER=offline
# 可选：自定义缓存，默认不会读取或修改 Paseo 的模型目录。
export AIT_SPEECH_MODELS_DIR=/absolute/path/to/models/local-speech
export AIT_SPEECH_OFFLINE_STT_MODEL=sensevoice-int8
export AIT_SPEECH_OFFLINE_TTS_MODEL=kokoro-multi-lang-v1_0
```

沿用 Paseo 的 Sherpa 模型格式和自动准备方式，同时支持其英文预设
`parakeet-tdt-0.6b-v2-int8` / `kokoro-en-v0_19`。若明确配置了已有缓存目录，完整模型可以直接复用。
模型在后台下载到临时目录，校验必需文件后替换安装；压缩包限制 1 GiB，解压限制 3 GiB，拒绝链接和越界路径。
推理在 daemon 自身的私有子进程中执行；模型保持加载，取消或超时会终止并回收该子进程，后续请求按需重启。
临时录音和生成文件随请求删除，不上传到云端。

Codex CLI 0.157.1 的 app-server 实验协议包含实时对话，但没有独立的文件转写方法；当前登录方式实测返回
`realtime conversation requires API key auth`，因此默认使用离线回退。不会读取或复用 Codex 登录令牌。
需要 Provider HTTP 语音接口时可显式配置下方 OpenAI 兼容后端；已有显式选择继续优先于默认值。

OpenAI 兼容服务：

```sh
export AIT_SPEECH_PROVIDER=openai
export AIT_SPEECH_BASE_URL=https://api.openai.com/v1
export AIT_SPEECH_STT_MODEL=whisper-1
export AIT_SPEECH_TTS_MODEL=tts-1
export AIT_SPEECH_VOICE=alloy
# 通过运行环境注入 AIT_SPEECH_API_KEY；也支持 OPENAI_API_KEY 回退。
```

外部本地 CLI（保留已有配置）：

```sh
export AIT_SPEECH_PROVIDER=local
export AIT_SPEECH_WHISPER_BIN=/absolute/path/to/whisper-cli
export AIT_SPEECH_WHISPER_MODEL=/absolute/path/to/ggml-model.bin
export AIT_SPEECH_PIPER_BIN=/absolute/path/to/piper
export AIT_SPEECH_PIPER_MODEL=/absolute/path/to/voice.onnx
```

Piper 需要模型配套的 `voice.onnx.json` 和匹配语言的 voice；whisper 模型须支持录音语言。
此 CLI 模式的程序、模型由用户安装。可用 `AIT_SPEECH_STT_PROVIDER`
和 `AIT_SPEECH_TTS_PROVIDER` 分别覆盖公共选择，例如本地识别、云端合成，或只启用听写。
`disabled` 关闭对应能力。更改环境配置后重启 daemon；密钥不进入 config.json 或日志。

云端实现依据 [OpenAI 文件转写](https://developers.openai.com/api/docs/guides/speech-to-text)
和 [TTS](https://developers.openai.com/api/docs/guides/text-to-speech) 的 HTTP 协议；本地参数依据
[whisper.cpp CLI](https://github.com/ggml-org/whisper.cpp/blob/master/examples/cli/README.md)
和 [Piper CLI](https://github.com/OHF-Voice/piper1-gpl/blob/main/docs/CLI.md)。

## 消息

在 `/v1/ws` 的 hello 中协商需要的方法。所有音频为 Base64；推荐格式
`audio/pcm;rate=16000;bits=16;channels=1`，也支持完整 `audio/wav`（单声道 PCM16）。
PCM 支持 8000、16000、22050、24000、44100、48000Hz。压缩容器不支持。

| 方法                    | 信封    | params                          |
| ----------------------- | ------- | ------------------------------- |
| voice.mode.set.request  | request | enabled、agentId（启用时必填）  |
| voice.abort.request     | request | 空对象                          |
| voice.audio.chunk       | event   | audio、format、isLast           |
| voice.audio.played      | event   | id                              |
| dictation.stream.start  | event   | dictationId、format             |
| dictation.stream.chunk  | event   | dictationId、seq、audio、format |
| dictation.stream.finish | event   | dictationId、finalSeq           |
| dictation.stream.cancel | event   | dictationId                     |

听写流程：start → ack(-1) → chunk(seq 从0开始) → ack(最高连续序号) → finish →
finish.accepted → final。乱序包最多领先128片，相同包重发幂等；同序号不同内容报错。
finalSeq 为最后一片的序号，不是分片数量。finish 之后可补发缺片，120秒内仍缺片则报错。
重复 finish 在结果缓存存活期间返回同一个 final，不再次计费。cancel 幂等，无成功事件。

```json
{"type":"event","method":"dictation.stream.start","params":{"dictationId":"d1","format":"audio/pcm;rate=16000;bits=16"}}
{"type":"event","method":"dictation.stream.chunk","params":{"dictationId":"d1","seq":0,"audio":"AAABAA==","format":"audio/pcm;rate=16000;bits=16"}}
{"type":"event","method":"dictation.stream.finish","params":{"dictationId":"d1","finalSeq":0}}
```

上面的短音频仅演示编码，识别结果由后端决定。

语音先以 request 启用目标 Agent。response.result 含 accepted、enabled、agentId、error，
失败时可带 reasonCode/retryable。以下为通用后端路径：上传音频触发转写，成功后调用现有 Agent 文本执行，
等待最终回复并合成语音。未启用模式时只发转写结果。不要把听写结果自动当成已提交的消息。

```json
{
  "type": "request",
  "request_id": "mode1",
  "method": "voice.mode.set.request",
  "params": { "enabled": true, "agentId": "existing-agent-id" }
}
```

| 服务端 event.method              | params                                                           |
| -------------------------------- | ---------------------------------------------------------------- |
| voice.input.state                | isSpeaking                                                       |
| voice.transcription.result       | requestId、text、可选 language                                   |
| voice.audio.output               | audio、format、id、isVoiceMode、groupId、chunkIndex、isLastChunk |
| voice.session.closed             | 空对象；停止本地录音和播放                                       |
| voice.error                      | error、reasonCode、retryable                                     |
| dictation.stream.ack             | dictationId、ackSeq                                              |
| dictation.stream.finish.accepted | dictationId、timeoutMs                                           |
| dictation.stream.partial         | dictationId、text                                                |
| dictation.stream.final           | dictationId、text                                                |
| dictation.stream.error           | dictationId、error、reasonCode、retryable                        |

每片 audio.output 实际播放完后回传 voice.audio.played。至多四片待确认，30秒不确认报错。
新的 voice.input.state(isSpeaking=true) 表示插话，客户端应清除之前排队的语音；显式
abort/关闭模式也应立即停止客户端播放。生成的语音应在客户端标明为 AI 合成语音。

状态只属于当前物理连接，不持久化。重连需使用新流并重新发送录音，不能将旧 ack 当成
新连接的接收进度。partial 每至少2秒对累计 PCM 重识别，可能被后续文本修正；WAV 只发
最终结果。客户端应保存录音直到 final，并按 reasonCode/retryable 显示可恢复错误。

通用后端的 VAD 是简单振幅门限和600ms静音判断；Codex 原生语音使用自己的检测与打断。识别质量、延迟和语言覆盖取决于实际模型；
中英文混说可能产生识别误差。离线模型参考 [SenseVoice](https://k2-fsa.github.io/sherpa/onnx/sense-voice/pretrained.html)
和 [Kokoro](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html)。
