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
  class Context {
    state = "running";
    sampleRate = 48000;
    destination = {};
    close = close;
    createBuffer = (_channels: number, length: number, rate: number) => ({
      duration: length / rate,
      getChannelData: () => new Float32Array(length),
    });
    createBufferSource = () => ({ start: play, connect: vi.fn(), addEventListener: vi.fn() });
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
