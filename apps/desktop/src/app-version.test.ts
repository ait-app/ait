import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { displayAppVersion } from "./app-version";

const state = vi.hoisted(() => ({ directory: "" }));
vi.mock("electron", () => ({
  app: { getAppPath: () => state.directory, getVersion: () => "0.1.12" },
}));

afterEach(() => {
  if (state.directory) rmSync(state.directory, { recursive: true, force: true });
  state.directory = "";
});

it.each(["2026-10-25-01234567", "git-2026-10-21-abcdef01"])(
  "uses the packaged display identity %s",
  (displayVersion) => {
    state.directory = mkdtempSync(path.join(tmpdir(), "ait-app-version-"));
    writeFileSync(
      path.join(state.directory, "package.json"),
      JSON.stringify({ version: "2026.10.25-g01234567", displayVersion }),
    );
    expect(displayAppVersion()).toBe(displayVersion);
  },
);

it("retains the Electron version when no display metadata is available", () => {
  expect(displayAppVersion()).toBe("0.1.12");
});
