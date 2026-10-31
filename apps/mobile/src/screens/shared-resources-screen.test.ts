import { describe, expect, it } from "vitest";
import type { HubAccountSnapshot, HubShare } from "@ait/client/internal/hub";
import { selectIncomingShares } from "./shared-resources-screen";

describe("selectIncomingShares", () => {
  it("keeps an empty fallback reference stable for Zustand selectors", () => {
    expect(selectIncomingShares(null)).toBe(selectIncomingShares(null));
    expect(selectIncomingShares({ shares: null })).toBe(selectIncomingShares({}));
  });

  it("returns the Hub share snapshot unchanged", () => {
    const shares = [{ shareId: "share-1" }] as HubShare[];
    const snapshot = { shares } as Pick<HubAccountSnapshot, "shares">;
    expect(selectIncomingShares(snapshot)).toBe(shares);
  });
});
