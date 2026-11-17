import { describe, expect, test, vi } from "vitest";
import type { WebContents } from "electron";
import { PASEO_BROWSER_PROFILE_PARTITION } from "../browser-profile.js";
import {
  getPaseoBrowserIdForWebContents,
  getPaseoBrowserWorkspaceId,
  isPaseoBrowserWebviewAttach,
  preparePaseoBrowserWebContents,
  registerAttachedPaseoBrowser,
  registerBrowserWebviewNavigationGuards,
  unregisterPaseoBrowser,
  unregisterPaseoBrowserFromHost,
} from "./index.js";

const { openExternal } = vi.hoisted(() => ({ openExternal: vi.fn() }));

vi.mock("electron", () => ({
  shell: { openExternal },
  webContents: { fromId: vi.fn() },
}));

class FakeWebContents {
  private readonly listeners = new Map<
    string,
    (event: { preventDefault: () => void; isMainFrame?: boolean; url: string }) => void
  >();

  public on(
    event: string,
    listener: (event: { preventDefault: () => void; isMainFrame?: boolean; url: string }) => void,
  ): this {
    this.listeners.set(event, listener);
    return this;
  }

  public emit(
    event: string,
    payload: { preventDefault: () => void; isMainFrame?: boolean; url: string },
  ): void {
    this.listeners.get(event)?.(payload);
  }
}

class FakeRenderer {
  public constructor(public readonly id: number) {}

  public isDestroyed(): boolean {
    return false;
  }
}

class FakeBrowserGuest {
  private destroyedListener: (() => void) | null = null;
  private destroyed = false;

  public constructor(
    public readonly id: number,
    public readonly hostWebContents: FakeRenderer,
    public readonly session: object,
  ) {}

  public isDestroyed(): boolean {
    return this.destroyed;
  }

  public once(event: "destroyed", listener: () => void): void {
    expect(event).toBe("destroyed");
    this.destroyedListener = listener;
  }

  public destroy(): void {
    this.destroyed = true;
    this.destroyedListener?.();
  }
}

describe("browser webview attachment", () => {
  test("opens Google account navigation in the external browser", async () => {
    openExternal.mockResolvedValue(undefined);
    const webContents = new FakeWebContents();
    const preventDefault = vi.fn();
    registerBrowserWebviewNavigationGuards(webContents as unknown as WebContents);

    webContents.emit("will-navigate", {
      url: "https://accounts.google.com/ServiceLogin?continue=https://www.google.com/",
      preventDefault,
    });
    await Promise.resolve();

    expect(preventDefault).toHaveBeenCalledOnce();
    expect(openExternal).toHaveBeenCalledWith(
      "https://accounts.google.com/ServiceLogin?continue=https://www.google.com/",
    );
  });

  test("keeps ordinary Google pages inside the workspace browser", () => {
    const webContents = new FakeWebContents();
    const preventDefault = vi.fn();
    registerBrowserWebviewNavigationGuards(webContents as unknown as WebContents);

    webContents.emit("will-navigate", {
      url: "https://www.google.com/",
      preventDefault,
    });

    expect(preventDefault).not.toHaveBeenCalled();
  });

  test("accepts a remote profile only when it belongs to this host window", () => {
    const input = { src: "http://127.0.0.1:4322/", partition: "persist:ait-browser-relay-host" };
    expect(isPaseoBrowserWebviewAttach(input)).toBe(false);
    expect(isPaseoBrowserWebviewAttach(input, [input.partition])).toBe(true);
    expect(
      isPaseoBrowserWebviewAttach({ ...input, src: "file:///tmp/page.html" }, [input.partition]),
    ).toBe(false);
  });
  test("accepts only allowed URLs on the shared profile partition", () => {
    expect(
      isPaseoBrowserWebviewAttach({
        src: "https://example.com",
        partition: PASEO_BROWSER_PROFILE_PARTITION,
      }),
    ).toBe(true);
    expect(
      isPaseoBrowserWebviewAttach({
        src: "https://example.com",
        partition: "persist:ait-browser-tab-a",
      }),
    ).toBe(false);
    expect(
      isPaseoBrowserWebviewAttach({ src: "https://example.com", partition: "persist:foreign" }),
    ).toBe(false);
  });

  test("binds explicit browser identity to the renderer that hosts the guest", () => {
    const profileSession = {};
    const renderer = new FakeRenderer(1);
    const guest = new FakeBrowserGuest(101, renderer, profileSession);

    const registered = registerAttachedPaseoBrowser({
      browserId: "browser-a",
      workspaceId: "workspace-a",
      webContentsId: guest.id,
      sender: renderer,
      profileSession,
      findWebContents: () => guest,
    });

    expect(registered).toBe(true);
    expect(getPaseoBrowserIdForWebContents(guest)).toBe("browser-a");
    expect(getPaseoBrowserWorkspaceId("browser-a")).toBe("workspace-a");
    unregisterPaseoBrowser("browser-a");
  });

  test("rejects a guest hosted by another renderer", () => {
    const profileSession = {};
    const owner = new FakeRenderer(1);
    const claimant = new FakeRenderer(2);
    const guest = new FakeBrowserGuest(201, owner, profileSession);

    const registered = registerAttachedPaseoBrowser({
      browserId: "browser-rejected-owner",
      workspaceId: "workspace-a",
      webContentsId: guest.id,
      sender: claimant,
      profileSession,
      findWebContents: () => guest,
    });

    expect(registered).toBe(false);
    expect(getPaseoBrowserIdForWebContents(guest)).toBeNull();
  });

  test("rejects a guest outside the shared profile", () => {
    const profileSession = {};
    const renderer = new FakeRenderer(1);
    const guest = new FakeBrowserGuest(301, renderer, {});

    const registered = registerAttachedPaseoBrowser({
      browserId: "browser-rejected-profile",
      workspaceId: "workspace-a",
      webContentsId: guest.id,
      sender: renderer,
      profileSession,
      findWebContents: () => guest,
    });

    expect(registered).toBe(false);
    expect(getPaseoBrowserIdForWebContents(guest)).toBeNull();
  });

  test("concurrent windows cannot swap browser identities", () => {
    const profileSession = {};
    const firstRenderer = new FakeRenderer(1);
    const secondRenderer = new FakeRenderer(2);
    const firstGuest = new FakeBrowserGuest(401, firstRenderer, profileSession);
    const secondGuest = new FakeBrowserGuest(402, secondRenderer, profileSession);
    const guests = new Map([
      [firstGuest.id, firstGuest],
      [secondGuest.id, secondGuest],
    ]);

    registerAttachedPaseoBrowser({
      browserId: "browser-second",
      workspaceId: "workspace-second",
      webContentsId: secondGuest.id,
      sender: secondRenderer,
      profileSession,
      findWebContents: (id) => guests.get(id) ?? null,
    });
    registerAttachedPaseoBrowser({
      browserId: "browser-first",
      workspaceId: "workspace-first",
      webContentsId: firstGuest.id,
      sender: firstRenderer,
      profileSession,
      findWebContents: (id) => guests.get(id) ?? null,
    });

    expect(getPaseoBrowserIdForWebContents(firstGuest)).toBe("browser-first");
    expect(getPaseoBrowserIdForWebContents(secondGuest)).toBe("browser-second");
    unregisterPaseoBrowser("browser-first");
    unregisterPaseoBrowser("browser-second");
  });

  test("unregisters the same browser only from its requesting host", () => {
    const profileSession = {};
    const firstRenderer = new FakeRenderer(11);
    const secondRenderer = new FakeRenderer(22);
    const firstGuest = new FakeBrowserGuest(501, firstRenderer, profileSession);
    const secondGuest = new FakeBrowserGuest(502, secondRenderer, profileSession);

    for (const [renderer, guest] of [
      [firstRenderer, firstGuest],
      [secondRenderer, secondGuest],
    ] as const) {
      registerAttachedPaseoBrowser({
        browserId: "browser-shared-hosts",
        workspaceId: "workspace-shared",
        webContentsId: guest.id,
        sender: renderer,
        profileSession,
        findWebContents: () => guest,
      });
    }

    unregisterPaseoBrowserFromHost(firstRenderer.id, "browser-shared-hosts");

    expect(getPaseoBrowserIdForWebContents(firstGuest)).toBeNull();
    expect(getPaseoBrowserIdForWebContents(secondGuest)).toBe("browser-shared-hosts");
    expect(getPaseoBrowserWorkspaceId("browser-shared-hosts")).toBe("workspace-shared");
    unregisterPaseoBrowser("browser-shared-hosts");
  });

  test("removes registration when the guest is destroyed", () => {
    const profileSession = {};
    const renderer = new FakeRenderer(31);
    const guest = new FakeBrowserGuest(601, renderer, profileSession);
    preparePaseoBrowserWebContents(guest);
    registerAttachedPaseoBrowser({
      browserId: "browser-cleanup",
      workspaceId: "workspace-cleanup",
      webContentsId: guest.id,
      sender: renderer,
      profileSession,
      findWebContents: () => guest,
    });

    expect(getPaseoBrowserIdForWebContents(guest)).toBe("browser-cleanup");

    guest.destroy();

    expect(getPaseoBrowserIdForWebContents(guest)).toBeNull();
  });
});
