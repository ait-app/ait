import { beforeEach, describe, expect, it, vi } from "vitest";
import { openExternalUrl, openSmsUrl } from "./open-external-url";

const openUrl = vi.hoisted(() => vi.fn(async (_url: string) => {}));
const linkingOpenUrl = vi.hoisted(() => vi.fn(async (_url: string) => {}));
vi.mock("expo-linking", () => ({ openURL: linkingOpenUrl }));
vi.mock("@/constants/platform", () => ({ isWeb: true }));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => ({ opener: { openUrl } }) }));

beforeEach(() => {
  openUrl.mockClear();
  linkingOpenUrl.mockClear();
});

describe("SMS links", () => {
  it("opens an sms link in the system handler", async () => {
    await openSmsUrl("sms:+14155550100&body=ait-abcd");
    expect(linkingOpenUrl).toHaveBeenCalledWith("sms:+14155550100&body=ait-abcd");
  });

  it.each(["https://example.test", "tel:+14155550100", "javascript:alert(1)", "invalid"])(
    "ignores %s",
    async (url) => {
      await expect(openSmsUrl(url)).resolves.toBeUndefined();
      expect(linkingOpenUrl).not.toHaveBeenCalled();
    },
  );
});

describe("external URLs", () => {
  it("opens the email contact through the desktop host", async () => {
    await openExternalUrl("mailto:dong@necoex.com");
    expect(openUrl).toHaveBeenCalledWith("mailto:dong@necoex.com");
  });

  it.each([
    "file:///private/data",
    "javascript:alert(1)",
    "sms:+14155550100",
    "/relative",
    "invalid",
  ])("safely ignores %s for fire-and-forget callers", async (url) => {
    await expect(openExternalUrl(url)).resolves.toBeUndefined();
    expect(openUrl).not.toHaveBeenCalled();
  });
});
