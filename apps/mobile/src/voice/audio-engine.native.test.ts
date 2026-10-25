import { afterEach, describe, expect, it, vi } from "vitest";
import { createAudioEngine } from "./audio-engine.native";
import { createVoiceRuntime } from "./voice-runtime";

const native = vi.hoisted(() => ({
  initialize: vi.fn(async () => true),
  getMicrophonePermissionsAsync: vi.fn(async () => ({ granted: true })),
  requestMicrophonePermissionsAsync: vi.fn(async () => ({ granted: true })),
  toggleRecording: vi.fn((value: boolean) => value),
  resumePlayback: vi.fn(),
  playPCMData: vi.fn(),
  enqueuePCMData: vi.fn((_data: Uint8Array, _id: string) => true),
  stopPlayback: vi.fn(),
  tearDown: vi.fn(),
  releaseAudioSession: vi.fn(),
  listeners: new Map<string, (event: { id?: string; data?: unknown; error?: string }) => void>(),
  addExpoTwoWayAudioEventListener: vi.fn(
    (name: string, listener: (event: { id?: string; data?: unknown; error?: string }) => void) => {
      native.listeners.set(name, listener);
      return { remove: () => native.listeners.delete(name) };
    },
  ),
}));
vi.mock("@ait/expo-two-way-audio", () => native);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function fixture() {
  return createAudioEngine({ onCaptureData: vi.fn(), onVolumeLevel: vi.fn() });
}

function pcm() {
  return {
    size: 3840,
    type: "audio/pcm;rate=24000;bits=16;channels=1",
    arrayBuffer: async () => new ArrayBuffer(3840),
  };
}

afterEach(() => {
  vi.clearAllMocks();
  native.listeners.clear();
});

describe("native microphone cancellation", () => {
  it("releases initialized audio after denied microphone permission", async () => {
    const engine = fixture();
    await engine.initialize();
    native.getMicrophonePermissionsAsync.mockResolvedValueOnce({ granted: false });
    native.requestMicrophonePermissionsAsync.mockResolvedValueOnce({ granted: false });
    await expect(engine.startCapture()).rejects.toThrow("permission");
    expect(native.toggleRecording).not.toHaveBeenCalled();
    expect(native.releaseAudioSession).toHaveBeenCalled();
    await engine.destroy();
  });

  it.each(["permission", "initialization"])(
    "does not open the daemon when interrupted during %s",
    async (stage) => {
      const permission = deferred<{ granted: boolean }>();
      const initialization = deferred<boolean>();
      if (stage === "permission")
        native.getMicrophonePermissionsAsync.mockReturnValueOnce(permission.promise);
      else native.initialize.mockReturnValueOnce(initialization.promise);
      let stopping = Promise.resolve();
      const engine = createAudioEngine({
        onCaptureData: vi.fn(),
        onVolumeLevel: vi.fn(),
        onInterruption: () => {
          stopping = runtime.stopVoice();
        },
      });
      const runtime = createVoiceRuntime({
        engine,
        getServerInfo: () => null,
        supportsRealtimeVoice: () => true,
        activateKeepAwake: async () => {},
        deactivateKeepAwake: async () => {},
      });
      const setVoiceMode = vi.fn(async () => {});
      runtime.registerSession({
        serverId: "server",
        setVoiceMode,
        sendVoiceAudioChunk: async () => {},
        audioPlayed: async () => {},
        abortRequest: async () => {},
        setAssistantAudioPlaying: vi.fn(),
      });
      const starting = runtime.startVoice("server", "agent");
      await vi.waitFor(() =>
        expect(
          stage === "permission" ? native.getMicrophonePermissionsAsync : native.initialize,
        ).toHaveBeenCalledOnce(),
      );
      native.listeners.get("onAudioInterruption")!({ data: "began" });
      permission.resolve({ granted: true });
      initialization.resolve(true);
      await starting;
      await stopping;
      expect(setVoiceMode).not.toHaveBeenCalledWith(true, "agent");
      expect(native.toggleRecording).not.toHaveBeenCalledWith(true);
      expect(runtime.getSnapshot().phase).toBe("disabled");
      if (stage === "initialization") expect(native.releaseAudioSession).toHaveBeenCalled();
      await engine.destroy();
    },
  );

  it.each(["stop", "destroy"])(
    "does not record after %s while permission is pending",
    async (action) => {
      const permission = deferred<{ granted: boolean }>();
      native.getMicrophonePermissionsAsync.mockReturnValueOnce(permission.promise);
      const engine = fixture();
      const starting = engine.startCapture();
      if (action === "stop") await engine.stopCapture();
      else await engine.destroy();
      permission.resolve({ granted: true });
      await starting;
      expect(native.toggleRecording).not.toHaveBeenCalledWith(true);
      if (action === "stop") {
        await engine.startCapture();
        expect(native.toggleRecording).toHaveBeenCalledWith(true);
      }
      await engine.destroy();
    },
  );
});

describe("native continuous playback", () => {
  it("rejects unavailable device completion support instead of inventing a played confirmation", async () => {
    const engine = fixture();
    native.enqueuePCMData.mockReturnValueOnce(false);
    await expect(engine.play(pcm())).rejects.toThrow("unavailable");
    expect(engine.isPlaying()).toBe(false);
    expect(native.releaseAudioSession).toHaveBeenCalled();
    await engine.destroy();
  });

  it("queues all PCM ahead and only acknowledges device completion", async () => {
    const engine = fixture();
    const done = vi.fn();
    const playing = [engine.play(pcm()), engine.play(pcm()), engine.play(pcm())];
    for (const result of playing) void result.then(done);
    await vi.waitFor(() => expect(native.enqueuePCMData).toHaveBeenCalledTimes(3));
    expect(done).not.toHaveBeenCalled();
    for (const [, id] of native.enqueuePCMData.mock.calls) {
      native.listeners.get("onPlaybackComplete")!({ id });
    }
    expect(await Promise.all(playing)).toEqual([0.08, 0.08, 0.08]);
    await engine.destroy();
  });

  it("cancels pending decoding and scheduled PCM without acknowledging old callbacks", async () => {
    const engine = fixture();
    const decoding = deferred<ArrayBuffer>();
    const arrayBuffer = vi.fn(() => decoding.promise);
    const first = engine.play({ ...pcm(), arrayBuffer });
    const rejection = expect(first).rejects.toThrow("stopped");
    await vi.waitFor(() => expect(arrayBuffer).toHaveBeenCalledOnce());
    engine.stop();
    engine.clearQueue();
    decoding.resolve(new ArrayBuffer(3840));
    await rejection;
    const second = engine.play(pcm());
    await vi.waitFor(() => expect(native.enqueuePCMData).toHaveBeenCalledTimes(1));
    const id = native.enqueuePCMData.mock.calls[0][1];
    native.listeners.get("onPlaybackComplete")!({ id: "old-generation" });
    expect(engine.isPlaying()).toBe(true);
    native.listeners.get("onPlaybackComplete")!({ id });
    await second;
    await engine.destroy();
  });

  it.each(["play", "capture"])(
    "releases a late initialization after cancelling %s",
    async (action) => {
      const initialization = deferred<boolean>();
      native.initialize.mockReturnValueOnce(initialization.promise);
      const engine = fixture();
      const starting = action === "play" ? engine.play(pcm()) : engine.startCapture();
      const result = action === "play" ? expect(starting).rejects.toThrow("stopped") : starting;
      await vi.waitFor(() => expect(native.initialize).toHaveBeenCalledOnce());
      if (action === "play") engine.stop();
      else await engine.stopCapture();
      initialization.resolve(true);
      await result;
      await vi.waitFor(() => expect(native.releaseAudioSession).toHaveBeenCalled());
      expect(native.toggleRecording).not.toHaveBeenCalledWith(true);
      expect(native.enqueuePCMData).not.toHaveBeenCalled();
      await engine.destroy();
    },
  );

  it("cancels playback and capture on iOS interruption without resuming on ended", async () => {
    const onInterruption = vi.fn();
    const engine = createAudioEngine({
      onCaptureData: vi.fn(),
      onVolumeLevel: vi.fn(),
      onInterruption,
    });
    await engine.startCapture();
    const playing = engine.play(pcm());
    const rejected = expect(playing).rejects.toThrow("stopped");
    await vi.waitFor(() => expect(native.enqueuePCMData).toHaveBeenCalledOnce());
    native.listeners.get("onAudioInterruption")!({ data: "began" });
    await rejected;
    expect(onInterruption).toHaveBeenCalledOnce();
    native.listeners.get("onAudioInterruption")!({ data: "ended" });
    expect(native.toggleRecording).toHaveBeenCalledTimes(1);
    expect(engine.isPlaying()).toBe(false);
    await engine.destroy();
  });

  it("rejects device failure and keeps the queue bounded while callbacks are stalled", async () => {
    const engine = fixture();
    const playing = Array.from({ length: 32 }, () => engine.play(pcm()));
    const results = Promise.allSettled(playing);
    await expect(engine.play(pcm())).rejects.toThrow("unavailable");
    await vi.waitFor(() => expect(native.enqueuePCMData).toHaveBeenCalledTimes(32));
    const id = native.enqueuePCMData.mock.calls[0][1];
    native.listeners.get("onPlaybackComplete")!({ id, error: "device failed" });
    engine.stop();
    expect((await results).every((result) => result.status === "rejected")).toBe(true);
    await engine.destroy();
  });
});
