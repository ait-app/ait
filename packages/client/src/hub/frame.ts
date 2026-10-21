/** `ait-hub-v1` frame codec: `header_len:u16 BE | header JSON | payload`, one Binary message each. */

export const HUB_PROTOCOL = "ait-hub-v1";
export const HUB_PATH = "/v1/hub";
export const HUB_MAX_HEADER = 4 * 1024;
export const HUB_MAX_PAYLOAD = 1024 * 1024;

export type HubPayloadType = "text" | "binary";

/** Frames this client sends; sequenced kinds receive `seq` from the link. */
export type HubOutboundHeader =
  | { t: "hello"; v: 1; resume: { link: string; recv: number } | null }
  | { t: "ack"; a: number }
  | { t: "ping"; n: number }
  | { t: "open"; seq: number; s: string; to: { node: string } }
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
}

/** Frames the center sends. Unknown kinds are ignored for forward compatibility. */
export type HubInboundHeader =
  | HubWelcome
  | { t: "ack"; a: number }
  | { t: "pong"; n: number }
  | { t: "error"; code: string }
  | { t: "open"; seq: number; s: string; from: string }
  | { t: "accept"; seq: number; s: string }
  | { t: "msg"; seq: number; s?: string; from?: string; pt: HubPayloadType; ck?: string }
  | { t: "close"; seq: number; s: string; code: string }
  | { t: "presence"; seq: number };

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
        count(header.grace_seconds)
      );
    case "ack":
      return count(header.a);
    case "pong":
      return count(header.n);
    case "error":
      return text(header.code);
    case "open":
    case "accept":
    case "close":
      return count(header.seq) && text(header.s);
    case "msg":
      return (
        count(header.seq) &&
        (header.s === undefined || text(header.s)) &&
        (header.pt === "text" || header.pt === "binary")
      );
    case "presence":
      return count(header.seq);
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
