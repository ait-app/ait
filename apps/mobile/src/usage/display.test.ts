// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useUsageDisplay } from "./display";

// A plain function, not vi.fn: vitest spies attach their own handlers to returned promises,
// which would mark the rejection handled and hide the bug under test.
let saveCalls = 0;
function failingUpdateSettings(): Promise<void> {
  saveCalls += 1;
  return Promise.reject(new Error("disk full"));
}

vi.mock("@/hooks/use-settings", () => ({
  useAppSettings: () => ({
    settings: { usage: { displayAs: "used", pins: [], serverId: null } },
    updateSettings: failingUpdateSettings,
  }),
}));
vi.mock("./hosts", () => ({ useUsageHostId: () => undefined }));
vi.mock("./queries", () => ({ useUsageHostReports: () => [] }));

describe("useUsageDisplay", () => {
  it("does not leave a failed settings save as an unhandled rejection", async () => {
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);
    const { result } = renderHook(() => useUsageDisplay([]));

    act(() => {
      result.current.display.setDisplayAs("remaining");
      result.current.display.togglePin({ sourceId: "source", windowId: "window" });
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    process.off("unhandledRejection", unhandled);

    expect(saveCalls).toBe(2);
    expect(unhandled).not.toHaveBeenCalled();
  });
});
