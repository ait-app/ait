export type {
  DaemonTransport as Transport,
  DaemonTransportFactory as TransportFactory,
} from "../daemon-client-transport-types.js";

export type Payload = Record<string, unknown>;

export function object(value: unknown): Payload {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Invalid Rust daemon message object");
  }
  return value as Payload;
}

export function strings(value: unknown): string[] {
  if (!Array.isArray(value) || !value.every((item) => typeof item === "string")) {
    throw new Error("Invalid Rust daemon capabilities");
  }
  return value;
}
