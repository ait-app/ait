import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { JSDOM } from "jsdom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useAssistantImage } from "@/assistant-image/use-assistant-image";
import { UserMessageRemoteImage } from "./user-message-remote-image";

vi.mock("@/assistant-image/use-assistant-image", () => ({ useAssistantImage: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: unknown) => unknown) =>
      factory({ colors: { surface1: "#fff", foregroundMuted: "#888" } }),
  },
}));
vi.mock("./attachment-pill", () => ({
  AttachmentFrame: ({ onPress, children }: { onPress?: () => void; children: React.ReactNode }) => (
    <button onClick={onPress} disabled={!onPress}>
      {children}
    </button>
  ),
}));
vi.mock("./attachment-lightbox", () => ({
  AttachmentLightbox: ({ source }: { source: { uri?: string } | null }) =>
    source ? <div data-testid="lightbox" data-uri={source.uri} /> : null,
}));

let dom: JSDOM;
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost" });
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("HTMLElement", dom.window.HTMLElement);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  dom.window.close();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it("shows a remotely acquired preview and opens it in the image viewer", async () => {
  vi.mocked(useAssistantImage).mockReturnValue({
    status: "loaded",
    aspectRatio: 1,
    binding: {
      uri: "https://example.test/preview.png",
      onRef: vi.fn(),
      onLoad: vi.fn(),
      onError: vi.fn(),
    },
  });
  await act(async () =>
    root.render(
      <UserMessageRemoteImage source="/host/plant.png" occurrenceKey="user:1" serverId="host" />,
    ),
  );
  expect(useAssistantImage).toHaveBeenCalledWith(
    expect.objectContaining({
      source: "/host/plant.png",
      occurrenceKey: "user:1",
      serverId: "host",
    }),
  );
  expect(container.querySelector("img")?.getAttribute("src")).toBe(
    "https://example.test/preview.png",
  );
  await act(async () => container.querySelector("button")?.click());
  expect(container.querySelector('[data-testid="lightbox"]')?.getAttribute("data-uri")).toBe(
    "https://example.test/preview.png",
  );
});

it("keeps unavailable previews visible without opening an empty viewer", async () => {
  vi.mocked(useAssistantImage).mockReturnValue({ status: "failed", message: "Image unavailable" });
  await act(async () =>
    root.render(<UserMessageRemoteImage source="/host/missing.png" occurrenceKey="user:2" />),
  );
  expect(container.querySelector("button")?.disabled).toBe(true);
  expect(container.querySelector('[aria-label="Image unavailable"]')).not.toBeNull();
  expect(container.querySelector('[data-testid="lightbox"]')).toBeNull();
});
