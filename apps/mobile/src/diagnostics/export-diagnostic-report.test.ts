import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  platform: { OS: "web" },
  write: vi.fn(async () => {}),
  remove: vi.fn(async () => {}),
  share: vi.fn(async () => {}),
  available: vi.fn(async () => true),
}));
vi.mock("react-native", () => ({ Platform: mocks.platform }));
vi.mock("expo-file-system/legacy", () => ({
  cacheDirectory: "file:///cache/",
  EncodingType: { UTF8: "utf8" },
  writeAsStringAsync: mocks.write,
  deleteAsync: mocks.remove,
}));
vi.mock("expo-sharing", () => ({ shareAsync: mocks.share, isAvailableAsync: mocks.available }));
import { exportDiagnosticReport } from "./export-diagnostic-report";

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  mocks.platform.OS = "web";
});

describe("diagnostic file export", () => {
  it("downloads the complete UTF-8 report and releases browser resources", async () => {
    vi.useFakeTimers();
    const link = { href: "", download: "", click: vi.fn(), remove: vi.fn() };
    const append = vi.fn();
    const create = vi.fn(() => "blob:diagnostic");
    const revoke = vi.fn();
    vi.stubGlobal("document", { createElement: () => link, body: { appendChild: append } });
    vi.stubGlobal("URL", { createObjectURL: create, revokeObjectURL: revoke });
    await exportDiagnosticReport("完整诊断\nsecond line");
    expect(await (create.mock.calls[0] as unknown as [Blob])[0].text()).toBe(
      "完整诊断\nsecond line",
    );
    expect(link.download).toMatch(/^ait-diagnostics-.*\.txt$/);
    expect(link.click).toHaveBeenCalledOnce();
    expect(link.remove).toHaveBeenCalledOnce();
    vi.runAllTimers();
    expect(revoke).toHaveBeenCalledWith("blob:diagnostic");
  });

  it("shares a native file and deletes the temporary copy", async () => {
    mocks.platform.OS = "ios";
    await exportDiagnosticReport("sanitized evidence");
    expect(mocks.write).toHaveBeenCalledWith(
      expect.stringContaining("file:///cache/ait-diagnostics-"),
      "sanitized evidence",
      { encoding: "utf8" },
    );
    expect(mocks.share).toHaveBeenCalledOnce();
    expect(mocks.remove).toHaveBeenCalledOnce();
  });

  it("cleans up even when native sharing fails", async () => {
    mocks.platform.OS = "android";
    mocks.share.mockRejectedValueOnce(new Error("sharing failed"));
    await expect(exportDiagnosticReport("evidence")).rejects.toThrow("sharing failed");
    expect(mocks.remove).toHaveBeenCalledOnce();
  });
});
