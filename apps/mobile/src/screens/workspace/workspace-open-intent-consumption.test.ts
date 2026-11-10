import { describe, expect, it } from "vitest";
import { resolveOpenIntentConsumption } from "./workspace-open-intent-consumption";

const KEY = "server:workspace:agent:a1";

describe("resolveOpenIntentConsumption", () => {
  it("opens a new intent once and ignores re-runs while the param is still set", () => {
    const first = resolveOpenIntentConsumption(null, KEY);
    expect(first).toEqual({ consumedKey: KEY, shouldOpen: true });
    expect(resolveOpenIntentConsumption(first.consumedKey, KEY)).toEqual({
      consumedKey: KEY,
      shouldOpen: false,
    });
  });

  it("opens the same intent again after the param clears", () => {
    const opened = resolveOpenIntentConsumption(null, KEY);
    const cleared = resolveOpenIntentConsumption(opened.consumedKey, null);
    expect(cleared).toEqual({ consumedKey: null, shouldOpen: false });
    expect(resolveOpenIntentConsumption(cleared.consumedKey, KEY)).toEqual({
      consumedKey: KEY,
      shouldOpen: true,
    });
  });
});
