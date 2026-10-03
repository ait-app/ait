import { METHODS } from "../../../src/runtime/rust-daemon/methods";
import { eventMessage, responseMessage } from "../../../src/runtime/rust-daemon/messages";
import type { SessionInboundMessage, SessionOutboundMessage } from "@ait/protocol/messages";

type Frame = string | Buffer;
function json(frame: Frame): Record<string, any> | null {
  try {
    return JSON.parse(typeof frame === "string" ? frame : frame.toString("utf8"));
  } catch {
    return null;
  }
}

/** Decode actual Rust wire frames through the same aliases used by the browser adapter. */
export function createSessionMessageReaders() {
  const requests = new Map<string, { alias: string; request: Record<string, any> }>();
  return {
    client(frame: Frame): SessionInboundMessage | null {
      const value = json(frame);
      if (value?.type === "session") return value.message;
      if (value?.type !== "request" && value?.type !== "event") return null;
      const match = Object.entries(METHODS).find(([, spec]) => spec.method === value.method);
      if (!match) return null;
      const request = { type: match[0], ...value.params, requestId: value.request_id };
      if (value.type === "request") requests.set(value.request_id, { alias: match[0], request });
      return request as SessionInboundMessage;
    },
    server(frame: Frame): SessionOutboundMessage | null {
      const value = json(frame);
      if (value?.type === "session") return value.message;
      if (value?.type === "event")
        return eventMessage(value.method, value.params).message as SessionOutboundMessage;
      if (value?.type !== "response") return null;
      const pending = requests.get(value.request_id);
      if (!pending) return null;
      requests.delete(value.request_id);
      const response = METHODS[pending.alias]?.response;
      return response
        ? (responseMessage(response, value.request_id, value.result, pending.request)
            .message as SessionOutboundMessage)
        : null;
    },
  };
}
