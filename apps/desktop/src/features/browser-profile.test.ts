import { describe, expect, test } from "vitest";
import {
  applyBrowserGuestPermissionPolicy,
  clearPaseoBrowserProfile,
  getLegacyPaseoBrowserProfileSession,
  getPaseoBrowserProfileSessions,
  isBrowserGuestPermissionAllowed,
  listPaseoBrowserProfileGuests,
  readLegacyPaseoBrowserIds,
} from "./browser-profile.js";

class FakeProfileSession {
  public readonly storageClears: unknown[] = [];
  public cacheClears = 0;
  public authClears = 0;
  public storageClear: Promise<void> = Promise.resolve();

  public clearStorageData(options: unknown): Promise<void> {
    this.storageClears.push(options);
    return this.storageClear;
  }

  public clearCache(): Promise<void> {
    this.cacheClears += 1;
    return Promise.resolve();
  }

  public clearAuthCache(): Promise<void> {
    this.authClears += 1;
    return Promise.resolve();
  }
}

class FakeLiveGuest {
  public reloads = 0;

  public constructor(
    public readonly id: number,
    private readonly destroyed = false,
    private readonly reloadError: Error | null = null,
  ) {}

  public isDestroyed(): boolean {
    return this.destroyed;
  }

  public reload(): void {
    if (this.reloadError) {
      throw this.reloadError;
    }
    this.reloads += 1;
  }
}

class FakeWebContents extends FakeLiveGuest {
  public constructor(
    id: number,
    public readonly session: object,
    private readonly type: string,
    destroyed = false,
  ) {
    super(id, destroyed);
  }

  public getType(): string {
    return this.type;
  }
}

describe("listPaseoBrowserProfileGuests", () => {
  test("returns every live webview and popup in the shared profile", () => {
    const profileSession = {};
    const firstWindowGuest = new FakeWebContents(1, profileSession, "webview");
    const secondWindowGuest = new FakeWebContents(2, profileSession, "webview");
    const foreignProfileGuest = new FakeWebContents(3, {}, "webview");
    const popupWindow = new FakeWebContents(4, profileSession, "window");
    const destroyedGuest = new FakeWebContents(5, profileSession, "webview", true);

    const guests = listPaseoBrowserProfileGuests({
      profileSession,
      webContents: [
        firstWindowGuest,
        secondWindowGuest,
        foreignProfileGuest,
        popupWindow,
        destroyedGuest,
      ],
    });

    expect(guests).toEqual([firstWindowGuest, secondWindowGuest, popupWindow]);
  });
});

describe("legacy browser profiles", () => {
  test("accepts only unique saved browser ids and resolves their old partitions", () => {
    const uuid = "123e4567-e89b-42d3-a456-426614174000";
    const fallbackId = "1700000000000-abcd";
    const browserIds = readLegacyPaseoBrowserIds([uuid, fallbackId, uuid, "not-a-browser-id", 123]);
    const partitions: string[] = [];
    const sessions = getPaseoBrowserProfileSessions(
      {
        fromPartition: (partition) => {
          partitions.push(partition);
          return new FakeProfileSession();
        },
      },
      browserIds,
    );

    expect(partitions).toEqual([
      "persist:ait-browser",
      `persist:ait-browser-${uuid}`,
      `persist:ait-browser-${fallbackId}`,
    ]);
    expect(sessions).toHaveLength(3);
  });

  test("resolves one valid legacy profile for tab-close cleanup", () => {
    const partitions: string[] = [];
    const sessions = {
      fromPartition: (partition: string) => {
        partitions.push(partition);
        return new FakeProfileSession();
      },
    };

    expect(getLegacyPaseoBrowserProfileSession(sessions, "1700000000000-abcd")).not.toBeNull();
    expect(getLegacyPaseoBrowserProfileSession(sessions, "invalid")).toBeNull();
    expect(partitions).toEqual(["persist:ait-browser-1700000000000-abcd"]);
  });
});

describe("clearPaseoBrowserProfile", () => {
  test("clears site data, HTTP cache, and auth before reloading live guests", async () => {
    const profile = new FakeProfileSession();
    const legacyProfile = new FakeProfileSession();
    let finishStorageClear: (() => void) | null = null;
    profile.storageClear = new Promise((resolve) => {
      finishStorageClear = resolve;
    });
    const firstGuest = new FakeLiveGuest(1);
    const secondGuest = new FakeLiveGuest(2);

    const clearing = clearPaseoBrowserProfile({
      profileSessions: [profile, legacyProfile],
      listGuests: () => [firstGuest, secondGuest],
      logReloadError: () => {},
    });

    expect(firstGuest.reloads).toBe(0);
    expect(secondGuest.reloads).toBe(0);
    finishStorageClear?.();
    await clearing;

    expect(profile.storageClears).toEqual([
      {
        storages: [
          "cookies",
          "filesystem",
          "indexdb",
          "localstorage",
          "serviceworkers",
          "cachestorage",
          "shadercache",
        ],
      },
    ]);
    expect(profile.cacheClears).toBe(1);
    expect(profile.authClears).toBe(1);
    expect(legacyProfile.storageClears).toEqual(profile.storageClears);
    expect(legacyProfile.cacheClears).toBe(1);
    expect(legacyProfile.authClears).toBe(1);
    expect(firstGuest.reloads).toBe(1);
    expect(secondGuest.reloads).toBe(1);
  });

  test("skips destroyed guests and logs individual reload failures", async () => {
    const profile = new FakeProfileSession();
    const destroyedGuest = new FakeLiveGuest(1, true);
    const reloadError = new Error("guest disappeared");
    const failedGuest = new FakeLiveGuest(2, false, reloadError);
    const reloadErrors: Array<{ guestId: number; error: unknown }> = [];

    await clearPaseoBrowserProfile({
      profileSessions: [profile],
      listGuests: () => [destroyedGuest, failedGuest],
      logReloadError: (guestId, error) => reloadErrors.push({ guestId, error }),
    });

    expect(destroyedGuest.reloads).toBe(0);
    expect(failedGuest.reloads).toBe(0);
    expect(reloadErrors).toEqual([{ guestId: 2, error: reloadError }]);
  });

  test("propagates clear failures without reloading guests", async () => {
    const profile = new FakeProfileSession();
    const clearError = new Error("profile locked");
    profile.storageClear = Promise.reject(clearError);
    const guest = new FakeLiveGuest(1);

    await expect(
      clearPaseoBrowserProfile({
        profileSessions: [profile],
        listGuests: () => [guest],
        logReloadError: () => {},
      }),
    ).rejects.toBe(clearError);
    expect(guest.reloads).toBe(0);
  });
});

describe("browser guest permission policy", () => {
  test("denies device, clipboard-read and notification access unless explicitly allowed", () => {
    for (const permission of [
      "media",
      "clipboard-read",
      "deprecated-sync-clipboard-read",
      "notifications",
      "geolocation",
      "display-capture",
      "midi",
      "hid",
      "serial",
      "usb",
      "unknown",
    ]) {
      expect(isBrowserGuestPermissionAllowed(permission)).toBe(false);
    }
  });

  test("keeps page copy buttons and video fullscreen working", () => {
    expect(isBrowserGuestPermissionAllowed("clipboard-sanitized-write")).toBe(true);
    expect(isBrowserGuestPermissionAllowed("fullscreen")).toBe(true);
  });

  test("installs the same decision on both the request and the check handler", () => {
    let requestHandler!: (
      contents: unknown,
      permission: string,
      callback: (granted: boolean) => void,
    ) => void;
    let checkHandler!: (contents: unknown, permission: string) => boolean;
    applyBrowserGuestPermissionPolicy({
      setPermissionRequestHandler: (handler) => {
        requestHandler = handler;
      },
      setPermissionCheckHandler: (handler) => {
        checkHandler = handler;
      },
    });

    const granted: boolean[] = [];
    requestHandler(null, "media", (value) => granted.push(value));
    requestHandler(null, "fullscreen", (value) => granted.push(value));
    expect(granted).toEqual([false, true]);
    expect(checkHandler(null, "notifications")).toBe(false);
    expect(checkHandler(null, "clipboard-sanitized-write")).toBe(true);
  });
});
