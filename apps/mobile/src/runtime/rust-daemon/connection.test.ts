import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildAccountRelayClientConfig } from "./connection";
import { createAccountRelayTransportFactory } from "./account-transport";
import { parseAccountRelayTarget } from "./account-relay-target";
import { createRustDaemonTransportFactory } from "./transport";
import type { Transport } from "./types";

vi.mock("./account-transport", () => ({ createAccountRelayTransportFactory: vi.fn() }));
vi.mock("@/desktop/daemon/desktop-daemon-transport", () => ({
  buildDesktopDaemonTransportUrl: vi.fn(),
}));

const StandardURL = URL;
class AuthoritylessURL extends StandardURL {
  get hostname() {
    return super.protocol === "ait+desktop:" ? "" : super.hostname;
  }
  get pathname() {
    return super.protocol === "ait+desktop:"
      ? `//${super.hostname}${super.pathname}`
      : super.pathname;
  }
}
class SpecialOnlyURL extends StandardURL {
  constructor(url: string | URL, base?: string | URL) {
    if (/^ait\+desktop:/i.test(String(url))) throw new TypeError("Unsupported URL scheme");
    super(url, base);
  }
}

const hostId = "11111111-1111-4111-8111-111111111111";
const base = `ait+desktop://account-relay/${hostId}`;
const socket = (): Transport => ({
  send: vi.fn(),
  close: vi.fn(),
  onOpen: () => () => {},
  onClose: () => () => {},
  onError: () => () => {},
  onMessage: () => () => {},
});

describe.each([
  ["standard", StandardURL],
  ["authorityless non-special", AuthoritylessURL],
  ["special schemes only", SpecialOnlyURL],
] as const)("internal transport URLs with %s URL implementation", (_name, implementation) => {
  beforeEach(() => {
    vi.stubGlobal("URL", implementation);
    vi.mocked(createAccountRelayTransportFactory).mockImplementation(({ url }) => {
      parseAccountRelayTarget(url);
      return socket();
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("builds and opens one account relay channel with its saved service binding", () => {
    const config = buildAccountRelayClientConfig({
      hostId,
      center: "https://custom.test:9443/ait/api/",
    });
    expect(parseAccountRelayTarget(config.url)).toEqual({
      hostId,
      center: "https://custom.test:9443/ait/api",
    });
    const transport = config.transportFactory({ url: config.url });
    try {
      expect(createAccountRelayTransportFactory).toHaveBeenCalledExactlyOnceWith({
        url: config.url,
        headers: undefined,
      });
    } finally {
      transport.close();
    }
  });

  it.each([
    "ait+desktop://ssh?host=build-box",
    "ait+desktop://ssh/?host=build-box",
    "wss://example.test/v1/ws",
  ])("preserves four-channel transport for %s", (url) => {
    const connect = vi.fn(socket);
    const transport = createRustDaemonTransportFactory(connect)({ url });
    try {
      expect(connect).toHaveBeenCalledTimes(4);
      expect(connect).toHaveBeenCalledWith({ url, headers: undefined });
    } finally {
      transport.close();
    }
  });

  it.each([
    "ait+desktop://account-relay.evil.test/",
    "ait+desktop://account-relay@evil.test/",
    "ait+desktop://user:password@account-relay/",
    "ait+desktop://account-relay:443/",
    "ait+desktop://account-relay\\evil.test/",
    "ait+desktop://account-relay\n/",
    "ait+desktop://ssh.evil.test?host=build-box",
    "ait+desktop://ssh/extra?host=build-box",
    "ait+desktop://ssh?host=build-box#fragment",
    "wss://example.test/v1/ws?extra=value",
    "wss://user:password@example.test/v1/ws",
    "wss://example.test/v1/ws#fragment",
    "https://example.test/v1/ws",
  ])("rejects invalid endpoints before opening a channel: %s", (url) => {
    const connect = vi.fn(socket);
    expect(() => createRustDaemonTransportFactory(connect)({ url })).toThrow();
    expect(connect).not.toHaveBeenCalled();
  });

  it.each([
    base,
    `${base}?center=`,
    `${base}?center=https://custom.test&center=https://other.test`,
    `${base}?center=https://custom.test&token=secret`,
    `${base}?center=http://remote.test`,
    `${base}?center=https://user:password@remote.test`,
    `${base}?center=https://custom.test#fragment`,
    `${base}/extra?center=https://custom.test`,
    `ait+desktop://account-relay/extra/../${hostId}?center=https://custom.test`,
    "ait+desktop://account-relay/invalid?center=https://custom.test",
  ])("preserves account target validation: %s", (url) => {
    expect(() => parseAccountRelayTarget(url)).toThrow();
  });
});
