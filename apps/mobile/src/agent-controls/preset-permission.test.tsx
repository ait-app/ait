/** @vitest-environment jsdom */
import { renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { HostChoice } from "@/hosts/host-groups";

const state = vi.hoisted(() => ({ hosts: [] as HostChoice[] }));
vi.mock("@/runtime/host-runtime", () => ({ useHosts: () => state.hosts }));

import { usePresetPermissionDenied } from "./preset-permission";

it("only enables shared preset controls for the current whole-node grant", () => {
  const host = (scope?: "node" | "project" | "workspace", shareId = "grant"): HostChoice => ({
    serverId: "shared:grant",
    label: "shared host",
    connections: [
      {
        type: "accountRelay",
        id: "relay",
        center: "https://example.test",
        hostId: "h",
        shareId,
        shareResourceKind: scope,
      },
    ],
  });
  state.hosts = [];
  const { result, rerender } = renderHook(({ serverId }) => usePresetPermissionDenied(serverId), {
    initialProps: { serverId: "shared:grant" },
  });
  expect(result.current).toBe(true);
  for (const scope of [undefined, "workspace", "project"] as const) {
    state.hosts = [host(scope)];
    rerender({ serverId: "shared:grant" });
    expect(result.current).toBe(true);
  }
  state.hosts = [host("node", "other")];
  rerender({ serverId: "shared:grant" });
  expect(result.current).toBe(true);
  state.hosts = [host("node")];
  rerender({ serverId: "shared:grant" });
  expect(result.current).toBe(false);
  rerender({ serverId: "owner" });
  expect(result.current).toBe(false);
});
