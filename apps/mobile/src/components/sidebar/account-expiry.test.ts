import { describe, expect, it } from "vitest";
import { resolveAccountExpiry } from "./account-expiry";

const HOUR = 60 * 60 * 1000;
const DAY = 24 * HOUR;
const NOW = Date.parse("2026-11-22T00:00:00Z");

function expiringIn(ms: number) {
  return resolveAccountExpiry(new Date(NOW + ms).toISOString(), NOW);
}

describe("resolveAccountExpiry", () => {
  it("only names the scale while a week or more is left", () => {
    expect(expiringIn(95 * DAY).expiry).toEqual({ kind: "months" });
    expect(expiringIn(30 * DAY).expiry).toEqual({ kind: "months" });
    expect(expiringIn(30 * DAY - 1).expiry).toEqual({ kind: "weeks" });
    expect(expiringIn(7 * DAY).expiry).toEqual({ kind: "weeks" });
  });

  it("counts whole days between two days and a week", () => {
    expect(expiringIn(7 * DAY - 1).expiry).toEqual({ kind: "days", count: 6 });
    expect(expiringIn(2 * DAY).expiry).toEqual({ kind: "days", count: 2 });
  });

  it("counts whole hours under two days, and the last hour as one", () => {
    expect(expiringIn(2 * DAY - 1).expiry).toEqual({ kind: "hours", count: 47 });
    expect(expiringIn(90 * 60 * 1000).expiry).toEqual({ kind: "hours", count: 1 });
    expect(expiringIn(1000).expiry).toEqual({ kind: "hours", count: 1 });
  });

  it("reports an account whose expiry has passed as expired", () => {
    expect(expiringIn(0)).toEqual({ expiry: { kind: "expired" }, changesAt: null });
    expect(expiringIn(-DAY)).toEqual({ expiry: { kind: "expired" }, changesAt: null });
  });

  it("keeps an account without an expiry apart from one whose expiry is unknown", () => {
    expect(resolveAccountExpiry(null, NOW).expiry).toEqual({ kind: "never" });
    expect(resolveAccountExpiry(undefined, NOW).expiry).toEqual({ kind: "unknown" });
    expect(resolveAccountExpiry("not a date", NOW).expiry).toEqual({ kind: "unknown" });
  });

  it.each([
    ["months", 95 * DAY, { kind: "weeks" }],
    ["weeks", 20 * DAY, { kind: "days", count: 6 }],
    ["days", 5 * DAY + HOUR, { kind: "days", count: 4 }],
    ["the last two days", 2 * DAY + HOUR, { kind: "hours", count: 47 }],
    ["hours", 5 * HOUR + 1000, { kind: "hours", count: 4 }],
    ["the last hour", 30 * 60 * 1000, { kind: "expired" }],
  ])("reads differently at the moment it reports, counting down %s", (_name, offset, next) => {
    const expiresAt = new Date(NOW + offset).toISOString();
    const { expiry, changesAt } = resolveAccountExpiry(expiresAt, NOW);

    expect(resolveAccountExpiry(expiresAt, changesAt! - 1).expiry).toEqual(expiry);
    expect(resolveAccountExpiry(expiresAt, changesAt!).expiry).toEqual(next);
  });
});
