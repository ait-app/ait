// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  fetchAgentCommands,
  useAgentCommandsQuery,
  type AgentSlashCommand,
} from "./use-agent-commands-query";

const runtime = vi.hoisted(() => ({
  client: { listCommands: vi.fn() },
  context: { provider: "omp", cwd: "/repo", nativeSessionId: "one" },
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => runtime.client,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("./use-agent-command-context", () => ({ useAgentCommandContext: () => runtime.context }));
vi.mock("@/components/retained-panel", () => ({ useRetainedPanelActive: () => true }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let root: Root | undefined;
let host: HTMLElement | undefined;
let cache: QueryClient;
let ownership: AgentSlashCommand[] | undefined;
let readOwnership: () => AgentSlashCommand[] | undefined;
function Probe() {
  const result = useAgentCommandsQuery({ serverId: "host", agentId: "agent" });
  ownership = result.ownership;
  readOwnership = result.readOwnership;
  return null;
}
async function render() {
  if (!root) {
    host = document.createElement("div");
    root = createRoot(host);
    cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  }
  await act(async () => {
    root?.render(createElement(QueryClientProvider, { client: cache }, createElement(Probe)));
  });
}
interface CatalogResponse {
  requestId: string;
  agentId: string;
  error: string | null;
  commands: AgentSlashCommand[];
}
function payload(name?: string): CatalogResponse {
  return {
    requestId: "request",
    agentId: "agent",
    error: null,
    commands: name ? [{ name, description: "native", argumentHint: "", source: "plugin" }] : [],
  };
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  host?.remove();
  cache?.clear();
  runtime.context = { provider: "omp", cwd: "/repo", nativeSessionId: "one" };
  runtime.client = { listCommands: vi.fn() };
});

describe("command ownership discovery", () => {
  it("distinguishes unloaded ownership from a successful empty catalog", async () => {
    const { promise, resolve } = Promise.withResolvers<CatalogResponse>();
    runtime.client.listCommands.mockReturnValue(promise);
    await render();
    expect(ownership).toBeUndefined();
    await act(async () => resolve(payload()));
    await vi.waitFor(() => expect(ownership).toEqual([]));
  });

  it("revokes action-time ownership immediately on invalidation, before a rerender", async () => {
    runtime.client.listCommands.mockResolvedValue(payload());
    await render();
    await vi.waitFor(() => expect(readOwnership()).toEqual([]));
    const readAtClick = readOwnership;
    await act(async () => {
      const invalidation = cache.invalidateQueries({ refetchType: "none" });
      expect(readAtClick()).toBeUndefined();
      await invalidation;
    });
    runtime.client.listCommands.mockResolvedValue(payload("clear"));
    await act(async () => {
      await cache.refetchQueries();
    });
    expect(readAtClick()?.[0]?.name).toBe("clear");
  });

  it.each(["cwd", "nativeSessionId", "provider", "client"])(
    "fences a late catalog after %s changes",
    async (field) => {
      const { promise, resolve } = Promise.withResolvers<CatalogResponse>();
      runtime.client.listCommands.mockReturnValueOnce(promise);
      await render();
      if (field === "client")
        runtime.client = { listCommands: vi.fn().mockResolvedValue(payload("current")) };
      else {
        runtime.context = { ...runtime.context, [field]: "changed" };
        runtime.client.listCommands.mockResolvedValue(payload("current"));
      }
      await render();
      await vi.waitFor(() => expect(ownership?.[0]?.name).toBe("current"));
      await act(async () => resolve(payload("stale")));
      expect(ownership?.[0]?.name).toBe("current");
    },
  );

  it("does not interpret a failed discovery as an empty ownership catalog", async () => {
    runtime.client.listCommands.mockResolvedValue({ ...payload(), error: "catalog unavailable" });
    await expect(fetchAgentCommands({ client: runtime.client, agentId: "agent" })).rejects.toThrow(
      "catalog unavailable",
    );
  });
});
