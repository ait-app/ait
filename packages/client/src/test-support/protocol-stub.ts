import type { DaemonTransportFactory } from "../daemon-client-transport-types.js";
import type { DaemonConnection } from "../protocol/connection.js";

/**
 * Business API unit fixtures operate on SDK messages, independently of wire negotiation.
 * Keep their serialized fixtures at this test boundary. The real DaemonClient + raw
 * transport integration is exercised by protocol/connection.test.ts and upload tests.
 */
export function createProtocolStub(
  factory: DaemonTransportFactory,
  options: { url: string; headers?: Record<string, string> },
): DaemonConnection {
  const transport = factory({ url: options.url, headers: options.headers });
  return {
    ...transport,
    forget: () => {},
    send(message) {
      if (message instanceof Uint8Array || message instanceof ArrayBuffer) {
        transport.send(message);
      } else {
        transport.send(
          JSON.stringify(
            message.type === "hello" || message.type === "connection.ping"
              ? message
              : { type: "session", message },
          ),
        );
      }
    },
    onMessage(handler) {
      return transport.onMessage((data, binary) => {
        if (binary) {
          handler(data, true);
          return;
        }
        const raw = data && typeof data === "object" && "data" in data ? data.data : data;
        let decoded;
        try {
          decoded = JSON.parse(String(raw));
        } catch {
          /* Malformed text is covered by the wire protocol tests. */
          return;
        }
        handler(decoded.type === "session" ? decoded.message : decoded, false);
      });
    },
  };
}
