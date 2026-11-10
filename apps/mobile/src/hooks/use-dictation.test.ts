/**
 * @vitest-environment jsdom
 */
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioEngineCallbacks } from "@/voice/audio-engine-types";

const engine = vi.hoisted(() => ({
  callbacks: null as AudioEngineCallbacks | null,
  startCapture: vi.fn(),
  stopCapture: vi.fn(async () => undefined),
}));

vi.mock("@/voice/audio-engine", () => ({
  createAudioEngine: (callbacks: AudioEngineCallbacks) => {
    engine.callbacks = callbacks;
    return {
      initialize: async () => undefined,
      startCapture: engine.startCapture,
      stopCapture: engine.stopCapture,
      destroy: async () => undefined,
    };
  },
}));
vi.mock(
  "@/hooks/use-dictation-audio-source",
  async () => await import("./use-dictation-audio-source.native"),
);

import { useDictation } from "./use-dictation";

describe("useDictation native capture start", () => {
  beforeEach(() => {
    engine.callbacks = null;
    engine.startCapture.mockReset();
    engine.stopCapture.mockClear();
  });

  it("does not report recording when an interruption cancels the capture start", async () => {
    let finishCapture!: () => void;
    engine.startCapture.mockReturnValue(
      new Promise<void>((resolve) => {
        finishCapture = resolve;
      }),
    );
    const onError = vi.fn();
    const { result } = renderHook(() =>
      useDictation({ client: null, onTranscript: vi.fn(), onError }),
    );

    let starting!: Promise<void>;
    act(() => {
      starting = result.current.startDictation();
    });
    await vi.waitFor(() => expect(engine.startCapture).toHaveBeenCalledOnce());

    act(() => engine.callbacks?.onInterruption?.());
    await act(async () => {
      finishCapture();
      await starting;
    });

    expect(result.current.isRecording).toBe(false);
    expect(result.current.status).toBe("idle");
    expect(engine.stopCapture).toHaveBeenCalled();
    expect(onError).toHaveBeenCalledOnce();
  });

  it("reports recording after capture starts without interruption", async () => {
    engine.startCapture.mockResolvedValue(undefined);
    const { result } = renderHook(() => useDictation({ client: null, onTranscript: vi.fn() }));

    await act(async () => {
      await result.current.startDictation();
    });

    expect(result.current.isRecording).toBe(true);
  });
});
