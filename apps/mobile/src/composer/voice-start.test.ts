/** @vitest-environment jsdom */
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useComposerVoiceStart } from "./voice-start";

function setup(createVoiceAgent = vi.fn(async (): Promise<string | null> => "created-agent")) {
  const voice = {
    phase: "disabled" as const,
    isVoiceMode: false,
    isVoiceSwitching: false,
    isMuted: false,
    activeServerId: null,
    activeAgentId: null,
    startVoice: vi.fn(async () => undefined),
    stopVoice: vi.fn(async () => undefined),
    toggleMute: vi.fn(),
    isVoiceModeForAgent: vi.fn(() => false),
  };
  const onError = vi.fn();
  const hook = renderHook(
    ({ hasAgent, agentId, disabled }) =>
      useComposerVoiceStart({
        voice,
        serverId: "host",
        agentId,
        hasAgent,
        disabled,
        createVoiceAgent,
        onError,
      }),
    { initialProps: { hasAgent: false, agentId: "draft", disabled: false } },
  );
  return { ...hook, voice, onError, createVoiceAgent };
}

describe("starting voice from a composer", () => {
  it("waits for one empty Agent creation before starting capture, including repeated clicks", async () => {
    let complete!: (id: string) => void;
    const creation = new Promise<string>((resolve) => {
      complete = resolve;
    });
    const create = vi.fn(() => creation);
    const { result, voice, rerender } = setup(create);
    let started!: Promise<void>;
    await act(async () => {
      started = result.current.startVoice();
      await result.current.startVoice();
    });
    expect(result.current.isStarting).toBe(true);
    expect(voice.startVoice).not.toHaveBeenCalled();
    rerender({ hasAgent: false, agentId: "draft", disabled: false });
    await act(async () => {
      await result.current.startVoice();
    });
    expect(create).toHaveBeenCalledTimes(1);
    await act(async () => {
      complete("real-agent");
      await started;
    });
    expect(voice.startVoice).toHaveBeenCalledExactlyOnceWith("host", "real-agent");
    expect(result.current.isStarting).toBe(false);
  });

  it("reports creation failure and permits a retry without starting voice on the draft ID", async () => {
    const create = vi
      .fn(async () => "real-agent")
      .mockRejectedValueOnce(new Error("Create failed"));
    const { result, voice, onError } = setup(create);
    await act(async () => {
      await result.current.startVoice();
    });
    expect(onError).toHaveBeenCalledWith("Create failed");
    expect(voice.startVoice).not.toHaveBeenCalled();
    expect(result.current.isStarting).toBe(false);
    await act(async () => {
      await result.current.startVoice();
    });
    expect(voice.startVoice).toHaveBeenCalledWith("host", "real-agent");
  });

  it("reuses the created session when microphone startup fails", async () => {
    const { result, rerender, voice, createVoiceAgent, onError } = setup();
    voice.startVoice.mockRejectedValueOnce(new Error("Microphone denied"));
    await act(async () => {
      await result.current.startVoice();
    });
    expect(onError).toHaveBeenCalledWith("Microphone denied");
    rerender({ hasAgent: true, agentId: "created-agent", disabled: false });
    await act(async () => {
      await result.current.startVoice();
    });
    expect(createVoiceAgent).toHaveBeenCalledTimes(1);
    expect(voice.startVoice).toHaveBeenLastCalledWith("host", "created-agent");
  });

  it("does not capture when creation no longer targets the current screen", async () => {
    const { result, voice } = setup(vi.fn(async () => null));
    await act(async () => {
      await result.current.startVoice();
    });
    expect(voice.startVoice).not.toHaveBeenCalled();
  });

  it("blocks creation while disconnected or submitting", async () => {
    const { result, rerender, createVoiceAgent, voice } = setup();
    rerender({ hasAgent: false, agentId: "draft", disabled: true });
    await act(async () => {
      await result.current.startVoice();
    });
    expect(createVoiceAgent).not.toHaveBeenCalled();
    expect(voice.startVoice).not.toHaveBeenCalled();
  });
});
