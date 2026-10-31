import { describe, expect, it } from "vitest";
import { parseAccountRelayTarget } from "./account-relay-target";

const hostId = "11111111-1111-4111-8111-111111111111";
const base = `ait+desktop://account-relay/${hostId}`;

describe("saved account relay targets", () => {
  it("retains a custom service's port and API path", () => {
    const url = new URL(base);
    url.searchParams.set("center", "https://custom.test:9443/ait/api/");
    expect(parseAccountRelayTarget(url.toString())).toEqual({
      hostId,
      center: "https://custom.test:9443/ait/api",
    });
  });

  it("parses a guest share only when its share and serve node are both bound", () => {
    const url = new URL(base);
    url.searchParams.set("center", "https://custom.test/api");
    url.searchParams.set("share", "22222222-2222-4222-8222-222222222222");
    url.searchParams.set("node", "serve:alice-workstation");
    expect(parseAccountRelayTarget(url.toString())).toEqual({
      hostId,
      center: "https://custom.test/api",
      shareId: "22222222-2222-4222-8222-222222222222",
      nodeId: "serve:alice-workstation",
    });
  });

  it.each([
    base,
    `${base}?center=`,
    `${base}?center=https://custom.test&center=https://other.test`,
    `${base}?center=https://custom.test&token=secret`,
    `${base}?center=http://remote.test`,
    `${base}?center=https://user:password@remote.test`,
    `${base}?center=https://custom.test#fragment`,
    `ait+desktop://user:password@account-relay/${hostId}?center=https://custom.test`,
    "ait+desktop://account-relay/invalid?center=https://custom.test",
    `${base}?center=https://custom.test&share=22222222-2222-4222-8222-222222222222`,
    `${base}?center=https://custom.test&node=serve-node`,
    `${base}?center=https://custom.test&share=invalid&node=serve-node`,
  ])("rejects an unbound or invalid service target: %s", (url) => {
    expect(() => parseAccountRelayTarget(url)).toThrow();
  });
});
