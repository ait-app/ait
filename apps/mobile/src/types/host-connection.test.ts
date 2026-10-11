import { defaultHostAppearance } from "@/hosts/appearance";
import { describe, expect, it } from "vitest";
import {
  createRemoteSshHostConnection,
  createAccountRelayHostConnection,
  normalizeStoredHostProfile,
  orderHostsLocalFirst,
  resolveActiveHostServerId,
  upsertHostConnectionInProfiles,
  type HostConnection,
  type HostProfile,
} from "./host-connection";

function makeHost(serverId: string): HostProfile {
  return {
    serverId,
    label: serverId,
    appearance: defaultHostAppearance(),
    lifecycle: {},
    connections: [],
    preferredConnectionId: null,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

describe("orderHostsLocalFirst", () => {
  it("moves the local host to the first position", () => {
    const remote = makeHost("srv_remote");
    const local = makeHost("srv_local");
    const anotherRemote = makeHost("srv_another_remote");

    expect(orderHostsLocalFirst([remote, local, anotherRemote], "srv_local")).toEqual([
      local,
      remote,
      anotherRemote,
    ]);
  });

  it("preserves host order when the local host is missing", () => {
    const hosts = [makeHost("srv_remote"), makeHost("srv_another_remote")];

    expect(orderHostsLocalFirst(hosts, "srv_local")).toBe(hosts);
  });

  it("preserves host order when there is no local host", () => {
    const hosts = [makeHost("srv_remote"), makeHost("srv_another_remote")];

    expect(orderHostsLocalFirst(hosts, null)).toBe(hosts);
  });
});

describe("normalizeStoredHostProfile", () => {
  it("restores saved online-service hosts with their service binding and appearance", () => {
    const connection = createAccountRelayHostConnection({
      hostId: "11111111-1111-4111-8111-111111111111",
      center: "https://custom.test:9443/ait/api/",
    });
    const profile = normalizeStoredHostProfile({
      serverId: "srv_account",
      label: "Workstation",
      appearance: { color: "teal", badgeDisplay: "icon" },
      connections: [connection],
      preferredConnectionId: connection.id,
    });
    expect(profile).toMatchObject({
      label: "Workstation",
      appearance: { color: "teal", badgeDisplay: "icon" },
      connections: [{ ...connection, center: "https://custom.test:9443/ait/api" }],
      preferredConnectionId: connection.id,
    });
  });

  it.each([
    { hostId: "invalid", center: "https://custom.test/api" },
    { hostId: "11111111-1111-4111-8111-111111111111", center: "http://remote.test" },
  ])("rejects an invalid saved online-service connection: %j", (connection) => {
    expect(
      normalizeStoredHostProfile({
        serverId: "srv_account",
        connections: [{ type: "accountRelay", ...connection }],
      }),
    ).toBeNull();
  });
  it("loads direct TCP connections stored before TLS and password fields existed", () => {
    const profile = normalizeStoredHostProfile({
      serverId: "srv_old",
      label: "Old Host",
      connections: [
        {
          id: "direct:127.0.0.1:6767",
          type: "directTcp",
          endpoint: "127.0.0.1:6767",
        },
      ],
      preferredConnectionId: "direct:127.0.0.1:6767",
      createdAt: "2026-01-01T00:00:00.000Z",
      updatedAt: "2026-01-02T00:00:00.000Z",
    });

    expect(profile).not.toBeNull();
    expect(profile?.connections[0]).toEqual({
      id: "direct:localhost:6767",
      type: "directTcp",
      endpoint: "localhost:6767",
      useTls: false,
    });
    expect(profile?.connections[0]).not.toHaveProperty("password");
  });

  it("drops relay-only hosts without TLS", () => {
    const profile = normalizeStoredHostProfile({
      serverId: "srv_relay",
      connections: [
        {
          id: "relay:relay.example.com:80",
          type: "relay",
          relayEndpoint: "relay.example.com:80",
          daemonPublicKeyB64: "pubkey",
        },
      ],
    });

    expect(profile).toBeNull();
  });

  it("drops relay-only hosts with TLS", () => {
    const profile = normalizeStoredHostProfile({
      serverId: "srv_relay",
      connections: [
        {
          id: "relay:relay.example.com:443",
          type: "relay",
          relayEndpoint: "relay.example.com:443",
          useTls: true,
          daemonPublicKeyB64: "pubkey",
        },
      ],
    });

    expect(profile).toBeNull();
  });

  it("gives a host stored before appearance existed the default appearance", () => {
    const profile = normalizeStoredHostProfile({
      serverId: "srv_old",
      connections: [{ id: "socket:/tmp/ait.sock", type: "directSocket", path: "/tmp/ait.sock" }],
    });

    expect(profile?.appearance).toEqual({ color: "none", badgeDisplay: null });
  });

  it("loads a stored appearance the user chose", () => {
    const profile = normalizeStoredHostProfile({
      serverId: "srv_new",
      appearance: { color: "teal", badgeDisplay: "icon" },
      connections: [{ id: "socket:/tmp/ait.sock", type: "directSocket", path: "/tmp/ait.sock" }],
    });

    expect(profile?.appearance).toEqual({ color: "teal", badgeDisplay: "icon" });
  });

  it("normalizes stored Remote SSH connection parameters", () => {
    const profile = normalizeStoredHostProfile({
      serverId: "srv_ssh",
      connections: [
        {
          type: "remoteSsh",
          host: " deploy@example.com ",
          sshPort: 2222,
          daemonPort: 7777,
        },
      ],
    });

    expect(profile?.connections[0]).toEqual({
      id: "ssh:deploy%40example.com:2222:7777",
      type: "remoteSsh",
      host: "deploy@example.com",
      sshPort: 2222,
      daemonPort: 7777,
    });
  });
});

describe("createRemoteSshHostConnection", () => {
  it("keeps optional SSH settings absent", () => {
    expect(createRemoteSshHostConnection({ host: "build-box" })).toEqual({
      id: "ssh:build-box::",
      type: "remoteSsh",
      host: "build-box",
    });
  });

  it("rejects invalid SSH destinations and ports", () => {
    expect(() => createRemoteSshHostConnection({ host: "" })).toThrow("SSH host is required");
    expect(() => createRemoteSshHostConnection({ host: "bad host" })).toThrow(
      "SSH host is invalid",
    );
    expect(() => createRemoteSshHostConnection({ host: "build-box", sshPort: 70000 })).toThrow(
      "SSH port must be between 1 and 65535",
    );
    expect(() => createRemoteSshHostConnection({ host: "build-box", daemonPort: 0 })).toThrow(
      "Daemon port must be between 1 and 65535",
    );
  });
});

describe("upsertHostConnectionInProfiles", () => {
  const connection: HostConnection = {
    id: "socket:/tmp/ait.sock",
    type: "directSocket",
    path: "/tmp/ait.sock",
  };

  it("gives a newly discovered host the default appearance", () => {
    const [profile] = upsertHostConnectionInProfiles({
      profiles: [],
      serverId: "srv_new",
      connection,
    });

    expect(profile.appearance).toEqual({ color: "none", badgeDisplay: null });
  });

  it("keeps the appearance the user chose when the host reconnects", () => {
    const existing: HostProfile = {
      ...makeHost("srv_known"),
      appearance: { color: "amber", badgeDisplay: "hidden" },
      connections: [],
    };

    const [profile] = upsertHostConnectionInProfiles({
      profiles: [existing],
      serverId: "srv_known",
      connection,
    });

    expect(profile.appearance).toEqual({ color: "amber", badgeDisplay: "hidden" });
  });

  it("replaces a direct connection when its settings change", () => {
    const existingConnection: HostConnection = {
      id: "direct:example.test:6767",
      type: "directTcp",
      endpoint: "example.test:6767",
      useTls: false,
      password: "old-secret",
    };
    const existing: HostProfile = {
      ...makeHost("srv_known"),
      connections: [existingConnection],
      preferredConnectionId: existingConnection.id,
    };
    const replacement: HostConnection = {
      ...existingConnection,
      useTls: true,
      password: "new-secret",
    };

    const [profile] = upsertHostConnectionInProfiles({
      profiles: [existing],
      serverId: "srv_known",
      connection: replacement,
    });

    expect(profile.connections).toEqual([replacement]);
    expect(profile.preferredConnectionId).toBe(replacement.id);
  });
});

describe("resolveActiveHostServerId", () => {
  it("uses the selected host when one is set", () => {
    expect(
      resolveActiveHostServerId({
        selectedServerId: "srv_selected",
        localServerId: "srv_local",
        hosts: [makeHost("srv_local"), makeHost("srv_selected")],
        orderedHosts: [makeHost("srv_local"), makeHost("srv_selected")],
      }),
    ).toBe("srv_selected");
  });

  it("falls back to the local host when it is connected", () => {
    expect(
      resolveActiveHostServerId({
        selectedServerId: null,
        localServerId: "srv_local",
        hosts: [makeHost("srv_local"), makeHost("srv_remote")],
        orderedHosts: [makeHost("srv_local"), makeHost("srv_remote")],
      }),
    ).toBe("srv_local");
  });

  it("skips a stopped local daemon and uses the first connected host", () => {
    // Regression: a stopped local daemon's serverId persists but isn't in `hosts`.
    // Falling back to it would resolve the section to an unknown id ("host not found").
    expect(
      resolveActiveHostServerId({
        selectedServerId: null,
        localServerId: "srv_local_stopped",
        hosts: [makeHost("srv_remote")],
        orderedHosts: [makeHost("srv_remote")],
      }),
    ).toBe("srv_remote");
  });

  it("returns null when no hosts are connected", () => {
    expect(
      resolveActiveHostServerId({
        selectedServerId: null,
        localServerId: "srv_local_stopped",
        hosts: [],
        orderedHosts: [],
      }),
    ).toBeNull();
  });

  it("ignores a selected host that is not connected", () => {
    // A stale selection (e.g. the host was removed) must not be used unless it is
    // currently connected, or the section resolves to an unknown id ("host not found").
    expect(
      resolveActiveHostServerId({
        selectedServerId: "srv_stale_selection",
        localServerId: null,
        hosts: [makeHost("srv_remote")],
        orderedHosts: [makeHost("srv_remote")],
      }),
    ).toBe("srv_remote");
  });

  it("falls through a disconnected selection to the connected local host", () => {
    expect(
      resolveActiveHostServerId({
        selectedServerId: "srv_stale_selection",
        localServerId: "srv_local",
        hosts: [makeHost("srv_local"), makeHost("srv_remote")],
        orderedHosts: [makeHost("srv_local"), makeHost("srv_remote")],
      }),
    ).toBe("srv_local");
  });
});

// Desktop server ports may change, but its saved connection identity must not.
it("preserves the managed connection id across storage and replaces its endpoint", () => {
  const connection = {
    id: "desktop-managed-srv_local",
    type: "directTcp" as const,
    endpoint: "localhost:49123",
  };
  const profile = {
    ...makeHost("srv_local"),
    connections: [connection],
    preferredConnectionId: connection.id,
  };
  const restored = normalizeStoredHostProfile(profile);
  expect(restored?.connections[0].id).toBe(connection.id);
  const next = upsertHostConnectionInProfiles({
    profiles: [restored!],
    serverId: "srv_local",
    connection: { ...connection, endpoint: "localhost:49124" },
  });
  expect(next[0].connections).toEqual([{ ...connection, endpoint: "localhost:49124" }]);
  expect(next[0].preferredConnectionId).toBe(connection.id);
});

it("persists SSH server tokens separately from identity and updates rotated tokens", () => {
  const connection = createRemoteSshHostConnection({
    host: "box",
    daemonPort: 7316,
    password: "a".repeat(32),
  });
  expect(connection.id).toBe("ssh:box::");
  const profile = normalizeStoredHostProfile({ serverId: "server", connections: [connection] })!;
  expect(profile.connections[0]).toEqual(connection);
  const rotated = { ...connection, password: "b".repeat(32) };
  const updated = upsertHostConnectionInProfiles({
    profiles: [profile],
    serverId: "server",
    connection: rotated,
  });
  expect(updated[0].connections).toEqual([rotated]);
});

it("drops legacy relay connections while preserving authenticated direct connections", () => {
  const profile = normalizeStoredHostProfile({
    serverId: "server",
    label: "My host",
    connections: [
      { type: "relay", relayEndpoint: "retired.example:443", daemonPublicKeyB64: "key" },
      { type: "directTcp", endpoint: "localhost:7316", password: "test-token" },
    ],
    preferredConnectionId: "relay:retired.example:443",
  });
  expect(profile?.connections).toHaveLength(1);
  expect(profile?.connections[0]).toMatchObject({ type: "directTcp", password: "test-token" });
  expect(profile?.preferredConnectionId).toBe(profile?.connections[0]?.id);
});
