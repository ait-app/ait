import { describe, expect, it } from "vitest";
import { DEFAULT_SIDEBAR_ROW_ITEMS, isChecksHiddenByLegacyRowItem } from "./row-items";

describe("DEFAULT_SIDEBAR_ROW_ITEMS", () => {
  it("shows operational metadata but hides identity badges by default", () => {
    expect(DEFAULT_SIDEBAR_ROW_ITEMS).toEqual({
      branch: false,
      project: false,
      host: true,
      changeRequest: true,
      services: true,
      labels: true,
    });
  });
});

describe("isChecksHiddenByLegacyRowItem", () => {
  it("reports the one stored shape that means checks were switched off", () => {
    expect(isChecksHiddenByLegacyRowItem({ checks: false })).toBe(true);
  });

  it.each([[{ checks: true }], [{ services: false }], [{}], [null], ["checks"], [["checks"]]])(
    "leaves %s to the new setting's default",
    (value) => {
      expect(isChecksHiddenByLegacyRowItem(value)).toBe(false);
    },
  );
});
