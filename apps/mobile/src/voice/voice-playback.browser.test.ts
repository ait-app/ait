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

it("keeps consecutive native PCM packets contiguous despite delayed ended callbacks", async () => {
  const engine = createAudioEngine({ onCaptureData: vi.fn(), onVolumeLevel: vi.fn() });
  vi.spyOn(engine, "startCapture").mockResolvedValue(undefined);
  const starts: { at: number; duration: number; buffer: AudioBuffer; context: BaseAudioContext }[] =
    [];
  const createSource = AudioContext.prototype.createBufferSource;
  vi.spyOn(AudioContext.prototype, "createBufferSource").mockImplementation(
    function (this: AudioContext) {
      const source = createSource.call(this);
      source.addEventListener("ended", () => {
        const until = performance.now() + 20;
        while (performance.now() < until) {
          /* simulate a busy renderer */
        }
      });
      return source;
    },
  );
  const originalStart = AudioBufferSourceNode.prototype.start;
  const startSpy = vi.spyOn(AudioBufferSourceNode.prototype, "start").mockImplementation(function (
    this: AudioBufferSourceNode,
    when = 0,
  ) {
    starts.push({
      at: Math.max(when, this.context.currentTime),
      duration: this.buffer!.duration,
      buffer: this.buffer!,
      context: this.context,
    });
    originalStart.call(this, when);
  });
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
    const count = 24;
    for (let chunk = 0; chunk < count; chunk++) {
      const bytes = new Uint8Array(3840);
      const view = new DataView(bytes.buffer);
      for (let i = 0; i < bytes.length / 2; i++) {
        view.setInt16(
          i * 2,
          Math.round(8192 * Math.cos(((chunk * 1920 + i) * 2 * Math.PI * 437) / 24000)),
          true,
        );
      }
      runtime.handleAudioOutput("server", {
        id: `chunk-${chunk}`,
        groupId: `group-${chunk}`,
        chunkIndex: 0,
        isLastChunk: true,
        isVoiceMode: true,
        format: "audio/pcm;rate=24000;bits=16;channels=1",
        audio: btoa(String.fromCharCode(...bytes)),
      });
    }
    await vi.waitFor(() => expect(adapter.audioPlayed).toHaveBeenCalledTimes(count), {
      timeout: 5000,
    });
    const gaps = starts.slice(1).map((start, i) => start.at - starts[i].at - starts[i].duration);
    expect(starts).toHaveLength(count);
    expect(Math.max(...gaps), "PCM boundary gap in seconds").toBeLessThan(1 / 24000);
    expect(
      starts.at(-1)!.at - starts[0].at,
      "playback must not accumulate renderer delay",
    ).toBeCloseTo((count - 1) * 0.08, 4);
    // Render the scheduled packets and a single unbroken reference through the
    // actual playback graph's rate. Independent per-packet resampling must not
    // create spikes at boundaries of this continuous, non-zero waveform.
    startSpy.mockRestore();
    const rate = starts[0].context.sampleRate;
    const render = async (split: boolean) => {
      const context = new OfflineAudioContext(1, Math.round(count * 0.08 * rate), rate);
      const buffers = split
        ? starts.map((s) => s.buffer)
        : [context.createBuffer(1, count * 1920, 24000)];
      if (!split)
        starts.forEach((s, i) =>
          buffers[0].getChannelData(0).set(s.buffer.getChannelData(0), i * 1920),
        );
      buffers.forEach((buffer, i) => {
        const source = context.createBufferSource();
        source.buffer = buffer;
        source.connect(context.destination);
        source.start(split ? starts[i].at - starts[0].at : 0);
      });
      return (await context.startRendering()).getChannelData(0);
    };
    const packets = await render(true);
    const continuous = await render(false);
    let peakError = 0;
    for (let i = 0; i < packets.length; i++)
      peakError = Math.max(peakError, Math.abs(packets[i] - continuous[i]));
    expect(peakError, "packet-boundary waveform spike").toBeLessThan(1 / 32768);
  } finally {
    button.remove();
    await runtime.destroy();
  }
}, 10000);
