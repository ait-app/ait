import { describe, expect, it } from "vitest";
import { createRequire } from "node:module";

const { configureAccountRelayWebSocket } = createRequire(import.meta.url)(
  "../plugins/with-account-relay-websocket",
);

describe("Android native relay WebSocket configuration", () => {
  it("installs one interceptor before React Native starts and only removes Origin for authenticated relay visits", () => {
    const source =
      "class MainApplication {\n  override fun onCreate() {\n    super.onCreate()\n    loadReactNative(this)\n  }\n}";
    const configured = configureAccountRelayWebSocket(source);
    expect(configured.indexOf("setCustomClientBuilder")).toBeLessThan(
      configured.indexOf("loadReactNative"),
    );
    expect(configured).toContain('request.header("Authorization")?.startsWith("Bearer ")');
    expect(configured).toContain("/v1/relay/sessions/[0-9a-fA-F-]{36}/client");
    expect(configured).toContain(
      'if (nativeRelay) request.newBuilder().removeHeader("Origin").build() else request',
    );
    expect(configured).toContain("followRedirects(false).followSslRedirects(false)");
    expect(configureAccountRelayWebSocket(configured)).toBe(configured);
    expect(() => configureAccountRelayWebSocket("invalid template")).toThrow();
  });
});
