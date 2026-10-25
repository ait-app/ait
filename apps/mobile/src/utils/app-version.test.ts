import { afterEach, expect, it, vi } from "vitest";
import Constants from "expo-constants";
import appPackage from "../../package.json";
import { resolveAppVersion } from "./app-version";

vi.mock("expo-constants", () => ({ default: { expoConfig: { extra: {} } } }));

afterEach(() => {
  vi.unstubAllEnvs();
  Constants.expoConfig!.extra = {};
});

it("displays the nightly identity ahead of the release package version", () => {
  vi.stubEnv("EXPO_PUBLIC_AIT_BUILD_VERSION", "2026-10-25-01234567");
  expect(resolveAppVersion()).toBe("2026-10-25-01234567");
});

it("uses the Git identity embedded during the Expo export", () => {
  vi.stubEnv("EXPO_PUBLIC_AIT_BUILD_VERSION", "");
  Constants.expoConfig!.extra = { buildVersion: "git-2026-10-21-abcdef01" };
  expect(resolveAppVersion()).toBe("git-2026-10-21-abcdef01");
});

it("retains the package version for releases without a build override", () => {
  vi.stubEnv("EXPO_PUBLIC_AIT_BUILD_VERSION", "");
  expect(resolveAppVersion()).toBe(appPackage.version);
});
