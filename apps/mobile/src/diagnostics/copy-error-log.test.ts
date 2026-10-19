import { afterEach, expect, it, vi } from "vitest";
import { copyErrorLog } from "./copy-error-log.web";

afterEach(() => vi.unstubAllGlobals());

it("starts clipboard writing immediately and propagates permission errors", async () => {
  const writeText = vi.fn().mockRejectedValue(new Error("Permission denied"));
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const result = copyErrorLog("error chain");
  expect(writeText).toHaveBeenCalledWith("error chain");
  await expect(result).rejects.toThrow("Permission denied");
});

it("rejects cleanly when the browser has no clipboard API", async () => {
  vi.stubGlobal("navigator", {});
  await expect(copyErrorLog("error chain")).rejects.toThrow("unavailable");
});
