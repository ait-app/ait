import { describe, expect, it } from "vitest";
import { isLaunchableBundle } from "./launchable-bundle";

describe("isLaunchableBundle", () => {
  it("treats macOS application and installer bundles as launchable", () => {
    expect(isLaunchableBundle("/repo/Tool.app", "darwin")).toBe(true);
    expect(isLaunchableBundle("/repo/Tool.APP/", "darwin")).toBe(true);
    expect(isLaunchableBundle("/repo/Setup.pkg", "darwin")).toBe(true);
  });

  it("keeps ordinary directories and other platforms browsable", () => {
    expect(isLaunchableBundle("/repo/my.app.src", "darwin")).toBe(false);
    expect(isLaunchableBundle("/repo/src", "darwin")).toBe(false);
    expect(isLaunchableBundle("/repo/Tool.app", "linux")).toBe(false);
  });
});
