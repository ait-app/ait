---
id: ADR-064
status: accepted
date: 2026-09-28
paths:
  - crates/voice
---

# ADR-064：默认离线语音与模型准备

- 关联：ADR-042

## 背景

Desktop 的听写和语音对话已接到 Rust server，但默认禁用 STT/TTS，两个按钮均报
Speech backend unavailable。用户要求先验证现有 Codex Provider 能否提供语音，否则参考 Paseo 回退离线。
Codex 0.157.1 实验实时协议在现有登录方式下返回 requires API key auth；不适合充当独立转写接口。
Paseo 使用 Sherpa ONNX 自动准备 Parakeet/Kokoro 模型，Agent Provider 仍处理文本。

## 决策

1. `voice` 保持 STT、TTS、Agent 三个 port。新增实现 port 的离线适配器，默认替代 disabled；
   显式 OpenAI HTTP、外部 whisper.cpp/Piper、disabled 配置保持优先，不读取 Codex 登录令牌。
2. 默认选择支持中文的 SenseVoice int8 / Kokoro multi-lang v1_0，保留 Paseo 英文模型预设。
   缓存归 Ait data-dir 所有；只有显式 models-dir 才复用其他目录。模型不进入 Git 或应用安装包。
3. server 组合根启动后台准备，服务监听不等待模型下载。port 的 readiness 在录音/启用语音前返回
   Preparing 或 ModelDownload 等安全、可重试错误，避免录完才发现不可用。
4. 模型下载只使用固定官方 HTTPS URL；下载/解压大小、条目数、路径和文件类型有界。
   验证完整的新安装后才替换旧目录；失败保留旧安装，临时目录自动清理。
5. 官方 `sherpa-onnx` 安全 Rust API 静态链接 native 库，不在工作区引入 unsafe。
   server 可通过私有 `--speech-worker` 模式作为推理子进程：有界 JSON 控制、请求私有文件、
   每模型串行复用。取消/120 秒超时终止并回收进程，native 推理不阻塞服务器 reactor。
   此模式不创建数据库、监听端口或读取 Provider 凭证。

## 验证与限制

参见 [验证报告](../../archive/reports/clients/desktop-speech-terminal.md) 和 [操作说明](../../operations/speech.md)。
首次使用需下载约 637 MiB 的安装内容。模型语言覆盖和转写质量不等同于云端模型；混说可能有误差。
当前实机验证为 macOS ARM64，Linux x86_64 仍需发布 CI 验证静态依赖。
