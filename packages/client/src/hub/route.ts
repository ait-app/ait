/**
 * Where a Host is served when the center's data planes (DPs) serve different Hosts (`redirect`).
 * The center names the DP by its API base (`center`) and its Hub WebSocket URL (`hub_url`).
 */

/** A DP of the account's center: its API base, which identifies it, and its Hub URL. */
export interface HubRoute {
  center: string;
  hubUrl: string;
}

/** A `close{code:"redirect"}` target: the node to open and where, with its location epoch. */
export interface HubRedirectTarget extends HubRoute {
  node: string;
  epoch: number;
}

const LOOPBACK = new Set(["localhost", "127.0.0.1", "[::1]"]);

/**
 * `value` without trailing slashes when it is a URL with the `secure` scheme, or the `local` one
 * on a loopback host (local development, as `normalizeCenter` allows); otherwise null.
 */
function endpoint(value: unknown, secure: string, local: string): string | null {
  if (typeof value !== "string") return null;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (url.username || url.password || url.search || url.hash) return null;
  if (url.protocol !== secure && !(url.protocol === local && LOOPBACK.has(url.hostname)))
    return null;
  return url.toString().replace(/\/+$/, "");
}

/** The DP serving a Host, from its `hosts` entry; null when this link's DP serves it. */
export function hubRoute(value: { center?: unknown; hub_url?: unknown }): HubRoute | null {
  const center = endpoint(value.center, "https:", "http:");
  const hubUrl = endpoint(value.hub_url, "wss:", "ws:");
  return center && hubUrl ? { center, hubUrl } : null;
}

function endpointIdentity(value: string): string {
  const url = new URL(value);
  // Local clusters can advertise 127.0.0.1 while the account connects through localhost.
  if ((url.protocol === "http:" || url.protocol === "ws:") && url.hostname === "localhost") {
    url.hostname = "127.0.0.1";
    return url.toString();
  }
  return value;
}

export function sameHubRoute(a: HubRoute, b: HubRoute): boolean {
  return (
    endpointIdentity(a.center) === endpointIdentity(b.center) ||
    endpointIdentity(a.hubUrl) === endpointIdentity(b.hubUrl)
  );
}

/** A well-formed `redirect` of a `close` frame, or null. */
export function hubRedirect(value: unknown): HubRedirectTarget | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const redirect = value as Record<string, unknown>;
  const route = hubRoute(redirect);
  const { node, epoch } = redirect;
  if (!route || typeof node !== "string" || !node) return null;
  if (typeof epoch !== "number" || !Number.isSafeInteger(epoch) || epoch < 0) return null;
  return { ...route, node, epoch };
}
