import { expect, it } from "vitest";
import { formatAmount } from "./format";

it("keeps yuan and dollars distinct in legacy quota cards", () => {
  expect(formatAmount(12.01, "cny")).toBe("CN¥12.01");
  expect(formatAmount(12.01, "usd")).toBe("$12.01");
  expect(formatAmount(0, "cny")).toBe("CN¥0.00");
});
