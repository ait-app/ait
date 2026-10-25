import { afterEach, expect, it, vi } from "vitest";
import { userEvent } from "vitest/browser";
import { createAudioEngine } from "./audio-engine.web";
import { createVoiceRuntime, type VoiceSessionAdapter } from "./voice-runtime";

afterEach(() => vi.restoreAllMocks());

it.each([
  { format: "audio/pcm;rate=24000;bits=16;channels=1", rate: 24000 },
  { format: "audio/pcm;rate=16000;bits=16;channels=1", rate: 16000 },
  { format: "pcm", rate: 24000 },
])("plays $format through the voice runtime in a real AudioContext", async ({ format, rate }) => {
  const engine = createAudioEngine({
    onCaptureData: vi.fn(),
    onVolumeLevel: vi.fn(),
    onInterruption: vi.fn(),
  });
  // Exercise playback without asking for a microphone or capturing the test host.
  vi.spyOn(engine, "startCapture").mockResolvedValue(undefined);
  const start = vi.spyOn(AudioBufferSourceNode.prototype, "start");
  const errors = vi.spyOn(console, "error").mockImplementation(() => undefined);
  const adapter: VoiceSessionAdapter = {
    serverId: "server",
    setVoiceMode: vi.fn().mockResolvedValue(undefined),
    sendVoiceAudioChunk: vi.fn().mockResolvedValue(undefined),
    audioPlayed: vi.fn().mockResolvedValue(undefined),
    abortRequest: vi.fn().mockResolvedValue(undefined),
    setAssistantAudioPlaying: vi.fn(),
  };
  const runtime = createVoiceRuntime({
    engine,
    getServerInfo: () => null,
    supportsRealtimeVoice: () => true,
    activateKeepAwake: async () => undefined,
    deactivateKeepAwake: async () => undefined,
  });
  runtime.registerSession(adapter);
  const button = document.createElement("button");
  document.body.append(button);
  let starting: Promise<void> | undefined;
  button.onclick = () => {
    starting = runtime.startVoice("server", "agent");
  };
  try {
    await userEvent.click(button);
    await starting;
    const bytes = new Uint8Array(rate * 0.08 * 2);
    const samples = new DataView(bytes.buffer);
    for (let i = 0; i < bytes.length / 2; i++) {
      samples.setInt16(i * 2, Math.round(Math.sin((i * 2 * Math.PI * 440) / rate) * 4096), true);
    }
    runtime.handleAudioOutput("server", {
      id: "chunk",
      groupId: "group",
      chunkIndex: 0,
      isLastChunk: true,
      isVoiceMode: true,
      format,
      audio: btoa(String.fromCharCode(...bytes)),
    });
    await vi.waitFor(() => expect(adapter.audioPlayed).toHaveBeenCalledWith("chunk"));
    expect(errors.mock.calls).toEqual([]);
    expect(start).toHaveBeenCalledOnce();
    const source = start.mock.contexts[0];
    if (!(source instanceof AudioBufferSourceNode)) throw new Error("PCM playback never started");
    expect(source.buffer?.sampleRate).toBe(rate);
    expect(source.buffer?.duration).toBeCloseTo(0.08);
    expect(source.buffer?.getChannelData(0).some((sample) => Math.abs(sample) > 0.1)).toBe(true);
  } finally {
    button.remove();
    await runtime.destroy();
  }
});
