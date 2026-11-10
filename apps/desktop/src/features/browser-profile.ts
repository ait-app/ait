export const PASEO_BROWSER_PROFILE_PARTITION = "persist:ait-browser";
const LEGACY_BROWSER_ID_PATTERN =
  /^(?:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}|\d{13,}-[0-9a-f]+)$/i;
const MAX_LEGACY_BROWSER_PROFILES = 1000;
// Guest pages run with no prompt UI, so anything that reaches devices or user data is refused.
const BROWSER_GUEST_ALLOWED_PERMISSIONS = new Set(["clipboard-sanitized-write", "fullscreen"]);

const PASEO_BROWSER_STORAGE_TYPES = [
  "cookies",
  "filesystem",
  "indexdb",
  "localstorage",
  "serviceworkers",
  "cachestorage",
  "shadercache",
] as const;

interface BrowserProfileSession {
  clearStorageData(options: {
    storages: Array<(typeof PASEO_BROWSER_STORAGE_TYPES)[number]>;
  }): Promise<void>;
  clearCache(): Promise<void>;
  clearAuthCache(): Promise<void>;
}

interface BrowserProfileGuest {
  readonly id: number;
  isDestroyed(): boolean;
  reload(): void;
}

interface BrowserProfileWebContents extends BrowserProfileGuest {
  readonly session: object;
  getType(): string;
}

interface ListBrowserProfileGuestsInput {
  profileSession: object;
  webContents: BrowserProfileWebContents[];
}

interface ClearBrowserProfileInput {
  profileSessions: BrowserProfileSession[];
  listGuests(): BrowserProfileGuest[];
  logReloadError(guestId: number, error: unknown): void;
}

interface ElectronSessions {
  fromPartition(partition: string): BrowserProfileSession;
}

interface BrowserGuestPermissionSession {
  setPermissionRequestHandler(
    handler: (contents: unknown, permission: string, callback: (granted: boolean) => void) => void,
  ): void;
  setPermissionCheckHandler(handler: (contents: unknown, permission: string) => boolean): void;
}

export function isBrowserGuestPermissionAllowed(permission: string): boolean {
  return BROWSER_GUEST_ALLOWED_PERMISSIONS.has(permission);
}

export function applyBrowserGuestPermissionPolicy(profile: BrowserGuestPermissionSession): void {
  profile.setPermissionRequestHandler((_contents, permission, callback) => {
    callback(isBrowserGuestPermissionAllowed(permission));
  });
  profile.setPermissionCheckHandler((_contents, permission) =>
    isBrowserGuestPermissionAllowed(permission),
  );
}

export function getPaseoBrowserProfileSession(sessions: ElectronSessions): BrowserProfileSession {
  return sessions.fromPartition(PASEO_BROWSER_PROFILE_PARTITION);
}

export function readLegacyPaseoBrowserIds(input: unknown): string[] {
  if (!Array.isArray(input)) {
    return [];
  }
  const browserIds = new Set<string>();
  for (const value of input) {
    if (typeof value === "string" && LEGACY_BROWSER_ID_PATTERN.test(value)) {
      browserIds.add(value);
      if (browserIds.size >= MAX_LEGACY_BROWSER_PROFILES) {
        break;
      }
    }
  }
  return [...browserIds];
}

export function getPaseoBrowserProfileSessions(
  sessions: ElectronSessions,
  legacyBrowserIds: string[],
): [BrowserProfileSession, ...BrowserProfileSession[]] {
  return [
    getPaseoBrowserProfileSession(sessions),
    // COMPAT(browserProfile): added in v0.1.108; remove after 2027-01-15.
    ...legacyBrowserIds.map((browserId) =>
      sessions.fromPartition(`${PASEO_BROWSER_PROFILE_PARTITION}-${browserId}`),
    ),
  ];
}

export function getLegacyPaseoBrowserProfileSession(
  sessions: ElectronSessions,
  browserId: string,
): BrowserProfileSession | null {
  const [legacyBrowserId] = readLegacyPaseoBrowserIds([browserId]);
  return legacyBrowserId
    ? sessions.fromPartition(`${PASEO_BROWSER_PROFILE_PARTITION}-${legacyBrowserId}`)
    : null;
}

export function listPaseoBrowserProfileGuests(
  input: ListBrowserProfileGuestsInput,
): BrowserProfileGuest[] {
  return input.webContents.filter(
    (contents) =>
      !contents.isDestroyed() &&
      (contents.getType() === "webview" || contents.getType() === "window") &&
      contents.session === input.profileSession,
  );
}

export async function clearPaseoBrowserProfile(input: ClearBrowserProfileInput): Promise<void> {
  await Promise.all(
    input.profileSessions.flatMap((profileSession) => [
      profileSession.clearStorageData({ storages: [...PASEO_BROWSER_STORAGE_TYPES] }),
      profileSession.clearCache(),
      profileSession.clearAuthCache(),
    ]),
  );

  for (const guest of input.listGuests()) {
    if (guest.isDestroyed()) {
      continue;
    }
    try {
      guest.reload();
    } catch (error) {
      input.logReloadError(guest.id, error);
    }
  }
}
