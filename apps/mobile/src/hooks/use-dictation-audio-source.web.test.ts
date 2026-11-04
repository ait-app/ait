/**
 * @vitest-environment jsdom
 */
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/desktop/host", () => ({ isElectronRuntime: () => false }));

import { useDictationAudioSource } from "./use-dictation-audio-source.web";

function createFakeStream() {
  const track = { stop: vi.fn() };
  return { track, stream: { getTracks: () => [track] } as unknown as MediaStream };
}

class BrokenAudioContext {
  static instances: BrokenAudioContext[] = [];
  sampleRate = 48000;
  close = vi.fn(async () => undefined);
  constructor() {
    BrokenAudioContext.instances.push(this);
  }
  createMediaStreamSource(): never {
    throw new Error("MediaStreamAudioSourceNode unavailable");
  }
}

describe("useDictationAudioSource (web)", () => {
  let getUserMedia: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    BrokenAudioContext.instances = [];
    getUserMedia = vi.fn();
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: { getUserMedia },
    });
    vi.stubGlobal("AudioContext", BrokenAudioContext);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(navigator, "mediaDevices");
  });

  it("releases the microphone when unmounted while getUserMedia is pending", async () => {
    const { track, stream } = createFakeStream();
    let resolveStream: (value: MediaStream) => void = () => undefined;
    getUserMedia.mockReturnValue(
      new Promise<MediaStream>((resolve) => {
        resolveStream = resolve;
      }),
    );
    const onError = vi.fn();
    const { result, unmount } = renderHook(() =>
      useDictationAudioSource({ onPcmSegment: vi.fn(), onError }),
    );

    const startPromise = result.current.start();
    unmount();
    resolveStream(stream);

    await expect(startPromise).rejects.toMatchObject({ name: "AttemptCancelledError" });
    expect(track.stop).toHaveBeenCalled();
    expect(BrokenAudioContext.instances).toHaveLength(0);
    expect(onError).not.toHaveBeenCalled();
  });

  it("releases the microphone when neither capture path can start", async () => {
    const { track, stream } = createFakeStream();
    getUserMedia.mockResolvedValue(stream);
    vi.stubGlobal("MediaRecorder", undefined);
    const { result } = renderHook(() => useDictationAudioSource({ onPcmSegment: vi.fn() }));

    await expect(result.current.start()).rejects.toThrow("MediaRecorder unavailable");
    expect(track.stop).toHaveBeenCalled();
    expect(BrokenAudioContext.instances[0]?.close).toHaveBeenCalled();
  });
});
