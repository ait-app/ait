// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { PermissionResponses } from "@ait/client/internal/daemon-client";
import { useRetainedPermissions } from "./use-retained-permissions";

afterEach(cleanup);

it("keeps an unconfirmed approval when the snapshot drops it and removes it only with evidence", async () => {
  const request = { id: "request", provider: "test", name: "approve", kind: "tool" as const };
  const operations = new PermissionResponses({
    send: vi.fn().mockRejectedValue(new Error("lost")),
    definitive: () => false,
    check: async () => null,
  });
  const client = { permissionResponses: operations } as unknown as DaemonClient;
  operations.remember("agent", request);
  await operations.submit("agent", "request", { behavior: "allow" }, 60_000).catch(() => {});
  const view = renderHook(({ requests }) => useRetainedPermissions(client, "agent", requests), {
    initialProps: { requests: [request] },
  });
  view.rerender({ requests: [] });
  expect(view.result.current).toEqual([request]);
  await act(() => operations.check("agent", "request"));
  expect(view.result.current).toEqual([request]);
  act(() =>
    operations.confirm({
      agentId: "agent",
      requestId: "request",
      resolution: { behavior: "allow" },
    }),
  );
  expect(view.result.current).toEqual([]);
});
