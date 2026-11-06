/** `ait-hub-v1` frame codec: `header_len:u16 BE | header JSON | payload`, one Binary message each. */

export const HUB_PROTOCOL = "ait-hub-v1";
export const HUB_PATH = "/v1/hub";
export const HUB_MAX_HEADER = 4 * 1024;
export const HUB_MAX_PAYLOAD = 1024 * 1024;

export type HubPayloadType = "text" | "binary";

/**
 * Optional `ait-hub-v1` features this client implements. A link announces them in `hello.f`
 * only when the center advertised them over HTTP (`hub_features`); `welcome.f` is the result.
 */
export const HUB_FEATURES = ["lease", "hosts", "state", "credit", "download"] as const;
export type HubFeature = (typeof HUB_FEATURES)[number];

/** Features a bulk link (`lane: "bulk"`) must announce; it may announce no others. */
export const HUB_BULK_FEATURES: readonly HubFeature[] = ["credit", "download"];

/** `main` links carry everything; `bulk` links carry only download sessions. */
export type HubLane = "main" | "bulk";

/** Largest session window (`credit`); every message up to the payload limit fits it. */
export const HUB_MAX_WINDOW = 1024 * 1024;

/** Session kind: business RPC (default) or a file download stream. */
export type HubSessionKind = "rpc" | "download";

/** Visibility and focus of an access endpoint, sent in `state` frames. */
export interface HubPresenceState {
  visible: boolean;
  focus?: { host: string; agent?: string; terminal?: string };
}

/** Frames this client sends; sequenced kinds receive `seq` from the link. */
export type HubOutboundHeader =
  | { t: "hello"; v: 1; resume: { link: string; recv: number } | null; f?: string[] }
  | { t: "ack"; a: number }
  | { t: "ping"; n: number }
  | ({ t: "state" } & HubPresenceState)
  | { t: "credit"; s: string; c: number }
  | { t: "call"; seq: number; id: string; method: string }
  | {
      t: "open";
      seq: number;
      s: string;
      to: { node: string };
      share?: string;
      k?: HubSessionKind;
      w?: number;
    }
  | { t: "msg"; seq: number; s: string; pt: HubPayloadType }
  | { t: "close"; seq: number; s: string; code: string };

export interface HubWelcome {
  t: "welcome";
  v: number;
  link: string;
  resumed: boolean;
  recv: number;
  resume_token?: string;
  node_id: string;
  grace_seconds: number;
  ping_seconds: number;
  max_payload: number;
  center_methods?: string[];
  /** Enabled features; present only when `hello.f` was sent. */
  f?: string[];
  /** Default session window; present only when `credit` is enabled. */
  win?: number;
}

/** One Host of the account as pushed in `hosts` frames. */
export interface HubHost {
  host_id: string;
  node_id: string | null;
  server_id: string | null;
  instance_id: string | null;
  name: string;
  hostname: string | null;
  platform: string | null;
  app_version: string | null;
  state: "online" | "reconnecting" | "offline";
  offline_since: string | null;
  /** Features of the Host's current serve link; `[]` without one. */
  f: string[];
}

/** Frames the center sends. Unknown kinds are ignored for forward compatibility. */
export type HubInboundHeader =
  | HubWelcome
  | { t: "ack"; a: number }
  | { t: "pong"; n: number }
  | { t: "error"; code: string }
  | { t: "open"; seq: number; s: string; from: string }
  | { t: "accept"; seq: number; s: string; w?: number }
  | { t: "msg"; seq: number; s?: string; from?: string; pt: HubPayloadType; ck?: string }
  | { t: "close"; seq: number; s: string; code: string }
  | { t: "credit"; s: string; c: number }
  | { t: "hosts"; seq: number; ver: number; full?: boolean }
  | { t: "presence"; seq: number }
  | { t: "groups"; seq: number }
  | { t: "share_presence"; seq: number }
  | { t: "reply"; seq: number; id: string }
  | {
      t: "event";
      seq: number;
      sub: string;
      cursor: number;
      host_id: string;
      event_id: string;
      resource_id?: string;
      method: string;
    };

export interface HubFrame {
  header: HubInboundHeader;
  payload: Uint8Array;
}

const encoder = new TextEncoder();

export function encodeHubFrame(header: HubOutboundHeader, payload?: Uint8Array): Uint8Array {
  const json = encoder.encode(JSON.stringify(header));
  if (json.byteLength > HUB_MAX_HEADER) throw new Error("Hub header exceeds the size limit.");
  const length = payload?.byteLength ?? 0;
  if (length > HUB_MAX_PAYLOAD) throw new Error("Hub payload exceeds the size limit.");
  const frame = new Uint8Array(2 + json.byteLength + length);
  frame[0] = json.byteLength >> 8;
  frame[1] = json.byteLength & 0xff;
  frame.set(json, 2);
  if (payload) frame.set(payload, 2 + json.byteLength);
  return frame;
}

function count(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function text(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function valid(header: Record<string, unknown>): boolean {
  switch (header.t) {
    case "welcome":
      return (
        text(header.link) &&
        typeof header.resumed === "boolean" &&
        count(header.recv) &&
        (header.resume_token === undefined || text(header.resume_token)) &&
        text(header.node_id) &&
        count(header.grace_seconds) &&
        (header.center_methods === undefined ||
          (Array.isArray(header.center_methods) && header.center_methods.every(text))) &&
        (header.f === undefined || (Array.isArray(header.f) && header.f.every(text))) &&
        (header.win === undefined || count(header.win))
      );
    case "ack":
      return count(header.a);
    case "pong":
      return count(header.n);
    case "error":
      return text(header.code);
    case "accept":
      return count(header.seq) && text(header.s) && (header.w === undefined || count(header.w));
    case "open":
    case "close":
      return count(header.seq) && text(header.s);
    case "credit":
      return text(header.s) && count(header.c);
    case "hosts":
      return (
        count(header.seq) &&
        count(header.ver) &&
        (header.full === undefined || typeof header.full === "boolean")
      );
    case "msg":
      return (
        count(header.seq) &&
        (header.s === undefined || text(header.s)) &&
        (header.pt === "text" || header.pt === "binary")
      );
    case "presence":
    case "groups":
    case "share_presence":
      return count(header.seq);
    case "reply":
      return count(header.seq) && text(header.id);
    case "event":
      return (
        count(header.seq) &&
        text(header.sub) &&
        count(header.cursor) &&
        text(header.host_id) &&
        text(header.event_id) &&
        text(header.method) &&
        (header.resource_id === undefined ||
          header.resource_id === null ||
          text(header.resource_id))
      );
    default:
      return typeof header.t === "string";
  }
}

/** Decode one center frame; throws on malformed framing or headers. */
export function decodeHubFrame(data: ArrayBuffer | Uint8Array): HubFrame {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  if (bytes.byteLength < 2) throw new Error("Truncated hub frame.");
  const length = (bytes[0] << 8) | bytes[1];
  if (length > HUB_MAX_HEADER || bytes.byteLength < 2 + length)
    throw new Error("Invalid hub frame header length.");
  const header: unknown = JSON.parse(new TextDecoder().decode(bytes.subarray(2, 2 + length)));
  if (!header || typeof header !== "object" || Array.isArray(header))
    throw new Error("Invalid hub frame header.");
  if (!valid(header as Record<string, unknown>)) throw new Error("Invalid hub frame header.");
  return { header: header as HubInboundHeader, payload: bytes.subarray(2 + length) };
}
