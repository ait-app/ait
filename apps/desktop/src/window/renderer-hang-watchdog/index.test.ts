import { describe, expect, it, vi } from "vitest";

vi.mock("electron", () => ({}));

import { shouldRecoverFromHang } from ".";

describe("renderer-hang-watchdog", () => {
  describe("shouldRecoverFromHang", () => {
    const recoverable = {
      unresponsiveEvents: 2,
      recovering: false,
      msSinceLastRecovery: 120_000,
      consecutiveRecoveries: 0,
    };

    it("recovers once the unresponsive threshold is reached", () => {
      expect(shouldRecoverFromHang(recoverable)).toBe(true);
    });

    it("waits until the unresponsive threshold is reached", () => {
      expect(shouldRecoverFromHang({ ...recoverable, unresponsiveEvents: 1 })).toBe(false);
    });

    it("does not recover while a recovery is already in progress", () => {
      expect(shouldRecoverFromHang({ ...recoverable, recovering: true })).toBe(false);
    });

    it("respects the cooldown between recoveries", () => {
      expect(shouldRecoverFromHang({ ...recoverable, msSinceLastRecovery: 30_000 })).toBe(false);
    });

    it("stops recovering after the consecutive-recovery cap", () => {
      expect(shouldRecoverFromHang({ ...recoverable, consecutiveRecoveries: 3 })).toBe(false);
    });
  });
});
