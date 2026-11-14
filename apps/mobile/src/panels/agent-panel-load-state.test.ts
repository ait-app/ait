import { describe, expect, it } from "vitest";
import type { AgentScreenMissingState } from "@/hooks/use-agent-screen-state-machine";
import {
  isAgentNotFoundErrorMessage,
  reconcileMissingAgentStateWithPresentAgent,
  shouldFetchAgentForLookup,
  shouldRetryLookupOnConnectionChange,
} from "./agent-panel-load-state";

describe("reconcileMissingAgentStateWithPresentAgent", () => {
  it("clears lookup-only states once the agent record is present", () => {
    expect(reconcileMissingAgentStateWithPresentAgent({ kind: "resolving" })).toEqual({
      kind: "idle",
    });
    expect(
      reconcileMissingAgentStateWithPresentAgent({
        kind: "not_found",
        message: "Agent not found: agent-1",
      }),
    ).toEqual({ kind: "idle" });
  });

  it("preserves history sync errors while the agent record is present", () => {
    const state: AgentScreenMissingState = {
      kind: "error",
      message: "Failed to get logs: session is archived",
    };

    expect(reconcileMissingAgentStateWithPresentAgent(state)).toBe(state);
  });
});

describe("shouldFetchAgentForLookup", () => {
  it("fetches only from idle so a failed lookup waits for Retry", () => {
    expect(shouldFetchAgentForLookup("idle")).toBe(true);
    expect(shouldFetchAgentForLookup("loading")).toBe(false);
    expect(shouldFetchAgentForLookup("not_found")).toBe(false);
    expect(shouldFetchAgentForLookup("error")).toBe(false);
  });
});

describe("shouldRetryLookupOnConnectionChange", () => {
  it("retries a failed lookup only on the reconnect edge", () => {
    const retry = (wasConnected: boolean, isConnected: boolean) =>
      shouldRetryLookupOnConnectionChange({ tag: "error", wasConnected, isConnected });
    expect(retry(false, true)).toBe(true);
    expect(retry(true, true)).toBe(false);
    expect(retry(true, false)).toBe(false);
  });

  it("leaves a lookup that did not fail alone", () => {
    expect(
      shouldRetryLookupOnConnectionChange({
        tag: "not_found",
        wasConnected: false,
        isConnected: true,
      }),
    ).toBe(false);
  });
});

describe("isAgentNotFoundErrorMessage", () => {
  it("recognizes the daemon's missing-agent lookup error", () => {
    expect(isAgentNotFoundErrorMessage("Agent not found: agent-1")).toBe(true);
  });

  it("recognizes the daemon's RPC error text for a missing agent", () => {
    expect(isAgentNotFoundErrorMessage("Agent was not found")).toBe(true);
  });

  it("leaves other not-found failures to the retryable error view", () => {
    expect(isAgentNotFoundErrorMessage("Workspace not found: ws-1")).toBe(false);
    expect(isAgentNotFoundErrorMessage("host not found")).toBe(false);
    expect(isAgentNotFoundErrorMessage("Route not found")).toBe(false);
  });
});
