import { afterEach, expect, it, vi } from "vitest";
import { createAudioEngine } from "./audio-engine.web";

vi.mock("@/desktop/host", () => ({ isElectronRuntime: () => false }));

afterEach(() => vi.unstubAllGlobals());

function fixture() {
  const track = { stop: vi.fn(), onended: null as (() => void) | null };
  const stream = { getTracks: () => [track], getAudioTracks: () => [track] };
  const processor = {
    connect: vi.fn(),
    disconnect: vi.fn(),
    onaudioprocess: null as
      | ((event: { inputBuffer: { getChannelData(channel: number): Float32Array } }) => void)
      | null,
  };
  const close = vi.fn().mockResolvedValue(undefined);
  const play = vi.fn();
  const sources: {
    start: typeof play;
    stop: ReturnType<typeof vi.fn>;
    disconnect: ReturnType<typeof vi.fn>;
    ended?: () => void;
  }[] = [];
  const contexts: Context[] = [];
  class Context {
    constructor() {
      contexts.push(this);
    }
    state = "running";
    currentTime = 0;
    sampleRate = 48000;
    destination = {};
    close = close;
    createBuffer = (_channels: number, length: number, rate: number) => {
      const samples = new Float32Array(length);
      return { duration: length / rate, getChannelData: () => samples };
    };
    createBufferSource = () => {
      const source = {
        start: play,
        stop: vi.fn(),
        connect: vi.fn(),
        disconnect: vi.fn(),
        ended: undefined as (() => void) | undefined,
        addEventListener: (_event: string, ended: () => void) => {
          source.ended = ended;
        },
      };
      sources.push(source);
      return source;
    };
    createMediaStreamSource = () => ({ connect: vi.fn(), disconnect: vi.fn() });
    createScriptProcessor = () => processor;
    createGain = () => ({ gain: { value: 0 }, connect: vi.fn(), disconnect: vi.fn() });
  }
  const getUserMedia = vi.fn().mockResolvedValue(stream);
  vi.stubGlobal("window", { AudioContext: Context, isSecureContext: true });
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
  const callbacks = { onCaptureData: vi.fn(), onVolumeLevel: vi.fn(), onInterruption: vi.fn() };
  return {
    engine: createAudioEngine(callbacks),
    track,
    stream,
    processor,
    callbacks,
    close,
    getUserMedia,
    play,
    sources,
    contexts,
  };
}

it("captures mono PCM for the daemon, honors mute, and releases ended devices", async () => {
  const f = fixture();
  await f.engine.startCapture();
  const event = { inputBuffer: { getChannelData: () => new Float32Array(2048).fill(0.25) } };
  f.processor.onaudioprocess?.(event);
  expect(f.callbacks.onCaptureData).toHaveBeenCalledOnce();
  const bytes: Uint8Array = f.callbacks.onCaptureData.mock.calls[0][0];
  expect(bytes.length).toBe(Math.round(2048 / 3) * 2);
  expect(new DataView(bytes.buffer).getInt16(0, true)).toBe(8192);
  f.engine.toggleMute();
  f.processor.onaudioprocess?.(event);
  expect(f.callbacks.onCaptureData).toHaveBeenCalledOnce();
  f.track.onended?.();
  expect(f.callbacks.onInterruption).toHaveBeenCalledOnce();
  expect(f.track.stop).toHaveBeenCalledOnce();
  expect(f.close).toHaveBeenCalledOnce();
});

it("releases a late microphone grant when voice was stopped during permission", async () => {
  const f = fixture();
  let grant!: (stream: typeof f.stream) => void;
  f.getUserMedia.mockReturnValue(
    new Promise((resolve) => {
      grant = resolve;
    }),
  );
  const starting = f.engine.startCapture();
  await vi.waitFor(() => expect(f.getUserMedia).toHaveBeenCalledOnce());
  await f.engine.stopCapture();
  grant(f.stream);
  await starting;
  expect(f.track.stop).toHaveBeenCalledOnce();
  expect(f.callbacks.onCaptureData).not.toHaveBeenCalled();
  expect(f.callbacks.onInterruption).not.toHaveBeenCalled();
});

it("does not restart playback after a late audio decode when voice has stopped", async () => {
  const f = fixture();
  let finish!: (bytes: ArrayBuffer) => void;
  const bytes = new Promise<ArrayBuffer>((resolve) => {
    finish = resolve;
  });
  const playing = f.engine.play({
    size: 960,
    type: "audio/pcm;rate=24000",
    arrayBuffer: () => bytes,
  });
  const stopped = expect(playing).rejects.toThrow("Playback stopped");
  f.engine.stop();
  f.engine.clearQueue();
  finish(new ArrayBuffer(960));
  await stopped;
  expect(f.play).not.toHaveBeenCalled();
});

it("cancels active and future scheduled packets without acknowledging their late ended events", async () => {
  const f = fixture();
  const source = {
    size: 3840,
    type: "audio/pcm;rate=24000",
    arrayBuffer: async () => new ArrayBuffer(3840),
  };
  const first = f.engine.play(source);
  const second = f.engine.play(source);
  const stopped = Promise.all([
    expect(first).rejects.toThrow("Playback stopped"),
    expect(second).rejects.toThrow("Playback stopped"),
  ]);
  await vi.waitFor(() => expect(f.sources).toHaveLength(2));
  expect(f.play.mock.calls.map((call) => call[0])).toEqual([0.12, 0.2]);
  f.engine.stop();
  for (const scheduled of f.sources) {
    expect(scheduled.stop).toHaveBeenCalledOnce();
    expect(scheduled.disconnect).toHaveBeenCalledOnce();
    scheduled.ended?.();
  }
  await stopped;
  expect(f.engine.isPlaying()).toBe(false);
  const restarted = f.engine.play(source);
  await vi.waitFor(() => expect(f.sources).toHaveLength(3));
  expect(f.play).toHaveBeenLastCalledWith(0.12);
  f.sources[2].ended?.();
  await expect(restarted).resolves.toBe(0.08);
  await f.engine.destroy();
});

it("resumes promptly after an input gap without accumulating another startup buffer", async () => {
  const f = fixture();
  const source = {
    size: 3840,
    type: "audio/pcm;rate=24000",
    arrayBuffer: async () => new ArrayBuffer(3840),
  };
  const first = f.engine.play(source);
  await vi.waitFor(() => expect(f.sources).toHaveLength(1));
  f.sources[0].ended?.();
  await first;
  f.contexts[0].currentTime = 0.5;
  const second = f.engine.play(source);
  await vi.waitFor(() => expect(f.sources).toHaveLength(2));
  expect(f.play).toHaveBeenLastCalledWith(0.52);
  f.sources[1].ended?.();
  await second;
  await f.engine.destroy();
});

it("catches up during excess pure silence while preserving even quiet nonzero reply heads", async () => {
  const f = fixture();
  const pcm = (value: number) => {
    const samples = new Int16Array(1920).fill(value);
    return {
      size: samples.byteLength,
      type: "audio/pcm;rate=24000",
      arrayBuffer: async () => samples.buffer,
    };
  };
  const chunks = [pcm(8192), ...Array.from({ length: 8 }, () => pcm(0)), pcm(1)];
  const playing = chunks.map((chunk) => f.engine.play(chunk));
  // A network burst must not make us replay an entire stale silent interval.
  await vi.waitFor(() => expect(f.sources).toHaveLength(5));
  expect(f.play.mock.calls.at(-1)?.[0]).toBeCloseTo(0.44);
  f.sources.forEach((source) => source.ended?.());
  const durations = await Promise.all(playing);
  expect(durations[0]).toBe(0.08);
  expect(durations.at(-1)).toBe(0.08);
  expect(durations.filter((duration) => duration === 0)).toHaveLength(5);
  await f.engine.destroy();
});
