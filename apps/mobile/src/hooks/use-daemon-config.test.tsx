/** @vitest-environment jsdom */
import React, { type ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MutableDaemonConfig } from "@ait/protocol/messages";
import { daemonConfigQueryKey } from "@/data/daemon-config";
import { useDaemonConfig } from "./use-daemon-config";

const runtime = vi.hoisted(() => ({
  connected: true,
  client: {
    getDaemonConfig: vi.fn(),
    patchDaemonConfig: vi.fn(),
  },
}));

vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => runtime.client,
  useHostRuntimeIsConnected: () => runtime.connected,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const serverId = "server-1";
const queryKey = daemonConfigQueryKey(serverId);

const config: MutableDaemonConfig = {
  relay: { enabled: false },
  mcp: { injectIntoAgents: true },
  browserTools: { enabled: false },
  providers: { codex: { enabled: true } },
  metadataGeneration: { providers: [{ provider: "codex", enabled: true }] },
  autoArchiveAfterMerge: false,
  enableTerminalAgentHooks: false,
  appendSystemPrompt: "",
};

function makeWrapper(queryClient: QueryClient) {
  return function wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

async function mountHook() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(queryKey, config);
  const view = renderHook(() => useDaemonConfig(serverId), {
    wrapper: makeWrapper(queryClient),
  });
  return { queryClient, view };
}

describe("useDaemonConfig patchConfig", () => {
  beforeEach(() => {
    runtime.connected = true;
    runtime.client.getDaemonConfig.mockReset();
    runtime.client.patchDaemonConfig.mockReset();
    runtime.client.getDaemonConfig.mockResolvedValue({ config });
  });

  it("shows a patch before the round trip completes", async () => {
    const { queryClient, view } = await mountHook();
    const settled = Promise.withResolvers<{ config: MutableDaemonConfig }>();
    runtime.client.patchDaemonConfig.mockReturnValue(settled.promise);

    let pending = Promise.resolve<MutableDaemonConfig | undefined>(undefined);
    act(() => {
      pending = view.result.current.patchConfig({ providers: { codex: { enabled: false } } });
    });

    expect(queryClient.getQueryData<MutableDaemonConfig>(queryKey)?.providers.codex).toEqual({
      enabled: false,
    });

    settled.resolve({ config: { ...config, providers: { codex: { enabled: false } } } });
    await act(async () => {
      await pending;
    });
    expect(queryClient.getQueryData<MutableDaemonConfig>(queryKey)?.providers.codex).toEqual({
      enabled: false,
    });
  });

  it("restores the previous config when the patch is rejected", async () => {
    const { queryClient, view } = await mountHook();
    const failure = new Error("rejected");
    runtime.client.patchDaemonConfig.mockRejectedValue(failure);

    await act(async () => {
      await expect(
        view.result.current.patchConfig({ providers: { codex: { enabled: false } } }),
      ).rejects.toThrow("rejected");
    });

    expect(queryClient.getQueryData(queryKey)).toEqual(config);
  });

  it("keeps a newer push that lands while a patch is failing", async () => {
    const { queryClient, view } = await mountHook();
    const failure = Promise.withResolvers<{ config: MutableDaemonConfig }>();
    runtime.client.patchDaemonConfig.mockReturnValue(failure.promise);
    const pushed: MutableDaemonConfig = { ...config, appendSystemPrompt: "from a push" };

    let pending = Promise.resolve<MutableDaemonConfig | undefined>(undefined);
    act(() => {
      pending = view.result.current.patchConfig({ providers: { codex: { enabled: false } } });
    });
    act(() => {
      queryClient.setQueryData(queryKey, pushed);
    });
    failure.reject(new Error("rejected"));
    await act(async () => {
      await expect(pending).rejects.toThrow("rejected");
    });

    expect(queryClient.getQueryData(queryKey)).toEqual(pushed);
  });

  it("reports a dropped connection only after it settles", async () => {
    const { view } = await mountHook();
    expect(view.result.current.error).toBeNull();

    vi.useFakeTimers();
    try {
      // Reconnecting swaps the client and dips the flag; a banner here would read as
      // "saved, but broken" and clear itself a blink later.
      runtime.connected = false;
      await act(async () => {
        view.rerender();
      });
      expect(view.result.current.error).toBeNull();

      await act(async () => {
        vi.advanceTimersByTime(999);
      });
      expect(view.result.current.error).toBeNull();

      await act(async () => {
        vi.advanceTimersByTime(1);
      });
      expect(view.result.current.error?.message).toBe("workspace.terminal.hostDisconnected");
    } finally {
      vi.useRealTimers();
    }
  });
});
