import { describe, expect, it } from "vitest";
import { resolveSchedulesScreenBodyState } from "./schedules-screen-state";

describe("resolveSchedulesScreenBodyState", () => {
  it("shows the empty list when loading fails", () => {
    expect(
      resolveSchedulesScreenBodyState({
        loadState: { status: "loading" },
        showLoadError: true,
      }),
    ).toEqual({ kind: "empty" });
  });
});
