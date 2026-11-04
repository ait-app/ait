/** @vitest-environment jsdom */
import React, { type ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useRewindAgentMutation } from "./use-rewind-agent-mutation";

const mocks = vi.hoisted(() => ({
  toastError: vi.fn(),
  completeRewind: vi.fn(),
  fetchAgentTimeline: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/contexts/toast-context", () => ({
  useToast: () => ({ error: mocks.toastError }),
}));
vi.mock("./composer-restore", () => ({
  useRewindComposerRestore: () => ({ completeRewind: mocks.completeRewind }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: { getState: () => ({ sessions: {} }) },
}));
vi.mock("@/runtime/host-runtime", () => ({
  getHostRuntimeStore: () => ({ fetchAgentTimeline: mocks.fetchAgentTimeline }),
}));

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}
    >
      {children}
    </QueryClientProvider>
  );
}

describe("useRewindAgentMutation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  it("restores the composer when the timeline refresh fails after a successful rewind", async () => {
    const rewindAgent = vi.fn(async () => {});
    mocks.fetchAgentTimeline.mockRejectedValueOnce(new Error("timeline offline"));
    const client = { rewindAgent } as unknown as DaemonClient;

    const { result } = renderHook(
      () => useRewindAgentMutation({ serverId: "srv", agentId: "agt", messageId: "msg", client }),
      { wrapper },
    );

    await act(async () => {
      await result.current.rewindAgent({ mode: "conversation", rewoundText: "hello" });
    });

    expect(rewindAgent).toHaveBeenCalledWith("agt", "msg", "conversation");
    expect(mocks.fetchAgentTimeline).toHaveBeenCalledOnce();
    expect(mocks.completeRewind).toHaveBeenCalledWith("hello");
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("reports a failed rewind without restoring the composer", async () => {
    const rewindAgent = vi.fn(async () => {
      throw new Error("rewind rejected");
    });
    const client = { rewindAgent } as unknown as DaemonClient;

    const { result } = renderHook(
      () => useRewindAgentMutation({ serverId: "srv", agentId: "agt", messageId: "msg", client }),
      { wrapper },
    );

    await act(async () => {
      await expect(
        result.current.rewindAgent({ mode: "conversation", rewoundText: "hello" }),
      ).rejects.toThrow("rewind rejected");
    });

    expect(mocks.completeRewind).not.toHaveBeenCalled();
    expect(mocks.toastError).toHaveBeenCalledWith("rewind rejected");
  });
});
