export const RUST_PROTOCOL_VERSION = { major: 1, min_minor: 0, max_minor: 0 } as const;

/** Capability requested by clients that want the single-socket Rust transport. */
export const RUST_SINGLE_CONNECTION_CAPABILITY = "connection.single.v1";
/** Feature advertised by daemons that support the single-socket Rust transport. */
export const RUST_SINGLE_CONNECTION_FEATURE = "ait-rust-single-v1";

export interface RustHelloOptions {
  clientId: string;
  capabilities: readonly string[];
  requiredCapabilities?: readonly string[];
}

export interface RustServerInfo extends Record<string, unknown> {
  server_id: string;
}

interface RustProtocolVersion {
  major: number;
  minor: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isProtocolVersion(value: unknown): value is RustProtocolVersion {
  return (
    isRecord(value) &&
    Number.isInteger(value.major) &&
    Number.isInteger(value.minor) &&
    typeof value.major === "number" &&
    typeof value.minor === "number"
  );
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

/** Serialize the canonical Rust v1 hello envelope used by every client entry point. */
export function buildHello({
  clientId,
  capabilities,
  requiredCapabilities = [],
}: RustHelloOptions): string {
  return JSON.stringify({
    type: "hello",
    client_id: clientId,
    protocol: RUST_PROTOCOL_VERSION,
    capabilities: [...capabilities],
    required_capabilities: [...requiredCapabilities],
  });
}

/** Parse a Rust server_info envelope while retaining fields added by newer daemons. */
export function parseServerInfo(value: unknown): RustServerInfo | null {
  if (!isRecord(value) || value.type !== "server_info" || !isRecord(value.info)) return null;
  const info = value.info;
  if (typeof info.server_id !== "string") return null;
  if (info.protocol !== undefined && !isProtocolVersion(info.protocol)) return null;
  if (info.features !== undefined && !isStringArray(info.features)) return null;
  return info as RustServerInfo;
}

export function hasSupportedRustProtocol(info: RustServerInfo): boolean {
  const protocol = info.protocol;
  return isProtocolVersion(protocol) && protocol.major === 1 && protocol.minor === 0;
}

export function hasRustFeature(info: RustServerInfo, feature: string): boolean {
  return isStringArray(info.features) && info.features.includes(feature);
}
