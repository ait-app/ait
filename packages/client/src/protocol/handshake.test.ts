import { describe, expect, test } from "vitest";
import {
  buildHello,
  hasSupportedRustProtocol,
  parseServerInfo,
  RUST_SINGLE_CONNECTION_CAPABILITY,
  RUST_SINGLE_CONNECTION_FEATURE,
  RUST_PROTOCOL_VERSION,
} from "./handshake.js";

describe("Rust v1 handshake", () => {
  test("buildHello emits the canonical wire envelope", () => {
    expect(
      JSON.parse(
        buildHello({
          clientId: "desktop-readiness",
          capabilities: ["connection.ping"],
          requiredCapabilities: [RUST_SINGLE_CONNECTION_CAPABILITY],
        }),
      ),
    ).toEqual({
      type: "hello",
      client_id: "desktop-readiness",
      protocol: RUST_PROTOCOL_VERSION,
      capabilities: ["connection.ping"],
      required_capabilities: [RUST_SINGLE_CONNECTION_CAPABILITY],
    });
  });

  test("parseServerInfo validates the shared envelope and retains wire fields", () => {
    const info = parseServerInfo({
      type: "server_info",
      info: {
        server_id: "server",
        instance_id: "instance",
        protocol: { major: 1, minor: 0 },
        features: [RUST_SINGLE_CONNECTION_FEATURE],
        future_field: { retained: true },
      },
    });

    expect(info).toMatchObject({
      server_id: "server",
      instance_id: "instance",
      future_field: { retained: true },
    });
    expect(info && hasSupportedRustProtocol(info)).toBe(true);
  });

  test("parseServerInfo rejects malformed server identity and feature fields", () => {
    expect(parseServerInfo({ type: "server_info", info: {} })).toBeNull();
    expect(
      parseServerInfo({ type: "server_info", info: { server_id: "server", features: [1] } }),
    ).toBeNull();
    expect(parseServerInfo({ type: "response", info: { server_id: "server" } })).toBeNull();
  });
});
