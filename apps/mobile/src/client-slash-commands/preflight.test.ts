import { expect, it, vi } from "vitest";
import { checkCommandPreflight } from "./preflight";
import { AgentCommandSessionCatalogUnavailable } from "@/hooks/use-agent-commands-query";

it("checks a fresh catalog and never authorizes an incomplete or failed result", async () => {
  const fetchCatalog = vi.fn(async () => ({
    commands: [],
    ownershipApplicable: true,
    ownershipComplete: true,
  }));
  expect(
    await checkCommandPreflight({ supportsOwnership: false, fetchCatalog, isCurrent: () => true }),
  ).toEqual({ status: "ready", ownership: { kind: "legacy" } });
  expect(fetchCatalog).not.toHaveBeenCalled();
  expect(
    await checkCommandPreflight({ supportsOwnership: true, fetchCatalog, isCurrent: () => true }),
  ).toEqual({ status: "ready", ownership: { kind: "complete", commands: [] } });
  fetchCatalog.mockResolvedValueOnce({
    commands: [],
    ownershipApplicable: true,
    ownershipComplete: false,
  });
  expect(
    await checkCommandPreflight({ supportsOwnership: true, fetchCatalog, isCurrent: () => true }),
  ).toEqual({ status: "unavailable" });
  fetchCatalog.mockRejectedValueOnce(new Error("connection lost"));
  expect(
    await checkCommandPreflight({ supportsOwnership: true, fetchCatalog, isCurrent: () => true }),
  ).toEqual({ status: "unavailable" });
  fetchCatalog.mockRejectedValueOnce(new AgentCommandSessionCatalogUnavailable("unavailable"));
  expect(
    await checkCommandPreflight({ supportsOwnership: true, fetchCatalog, isCurrent: () => true }),
  ).toEqual({ status: "session-unavailable" });
});

it("discards a result when the input, session, connection or directory revision changes", async () => {
  const pending = Promise.withResolvers<{
    commands: [];
    ownershipApplicable: true;
    ownershipComplete: true;
  }>();
  let current = true;
  const result = checkCommandPreflight({
    supportsOwnership: true,
    fetchCatalog: () => pending.promise,
    isCurrent: () => current,
  });
  current = false;
  pending.resolve({ commands: [], ownershipApplicable: true, ownershipComplete: true });
  expect(await result).toEqual({ status: "stale" });
});
