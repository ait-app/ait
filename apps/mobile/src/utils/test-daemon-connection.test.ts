import type { DaemonClientConfig } from "@ait/client/internal/daemon-client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  connectToDaemon,
  type DaemonConnectionDependencies,
  type DaemonProbeClient,
} from "./test-daemon-connection";

class FakeDaemonClient implements DaemonProbeClient {
  readonly lastError: string | null;

  constructor(
    private readonly probe: FakeDaemonProbe,
    readonly config: DaemonClientConfig,
  ) {
    this.lastError = probe.nextLastError;
  }

  async connect(): Promise<void> {
    if (this.probe.nextConnectError) {
      throw this.probe.nextConnectError;
    }
  }

  getLastServerInfoMessage() {
    return {
      serverId: "srv_probe_test",
      hostname: "probe-host",
    };
  }

  async close(): Promise<void> {
    this.probe.closedClients.push(this);
  }
}

class FakeDaemonProbe {
  createdClients: FakeDaemonClient[] = [];
  closedClients: FakeDaemonClient[] = [];
  clientIdsRequested = 0;
  nextConnectError: Error | null = null;
  nextLastError: string | null = null;

  readonly deps: DaemonConnectionDependencies<FakeDaemonClient> = {
    getClientId: async () => {
      this.clientIdsRequested += 1;
      return "cid_shared_probe_test";
    },
    resolveAppVersion: () => null,
    createDesktopTransportFactory: () => null,
    buildDesktopTransportUrl: (target) => {
      if (target.transportType === "rustTcp") return target.url;
      if (target.transportType === "ssh") {
        return `ait+desktop://ssh?host=${encodeURIComponent(target.host)}`;
      }
      return `ait+desktop://${target.transportType}?path=${encodeURIComponent(target.transportPath)}`;
    },
    createClient: (config) => {
      const client = new FakeDaemonClient(this, config);
      this.createdClients.push(client);
      return client;
    },
  };

  failNextConnection(error: Error, lastError: string | null): void {
    this.nextConnectError = error;
    this.nextLastError = lastError;
  }

  createdConfigs(): DaemonClientConfig[] {
    return this.createdClients.map((client) => client.config);
  }
}

describe("test-daemon-connection connectToDaemon", () => {
  let probe: FakeDaemonProbe;

  beforeEach(() => {
    vi.stubGlobal("__DEV__", false);
    probe = new FakeDaemonProbe();
  });

  it("reuses the app clientId for direct connections", async () => {
    const first = await connectToDaemon(
      {
        id: "direct:lan:6767",
        type: "directTcp",
        endpoint: "lan:6767",
      },
      undefined,
      probe.deps,
    );
    await first.client.close();

    const second = await connectToDaemon(
      {
        id: "direct:lan:6767",
        type: "directTcp",
        endpoint: "lan:6767",
      },
      undefined,
      probe.deps,
    );
    await second.client.close();

    const [firstConfig, secondConfig] = probe.createdConfigs();
    expect(firstConfig?.clientId).toBe("cid_shared_probe_test");
    expect(secondConfig?.clientId).toBe("cid_shared_probe_test");
    expect(probe.clientIdsRequested).toBe(2);
  });

  it("adapts direct TCP probes to the Rust protocol", async () => {
    const result = await connectToDaemon(
      {
        id: "direct:lan:6767",
        type: "directTcp",
        endpoint: "lan:6767",
      },
      undefined,
      probe.deps,
    );
    await result.client.close();

    expect(probe.createdConfigs()[0]?.transportFactory).toBeTypeOf("function");
    expect(probe.createdConfigs()[0]?.url).toBe("ws://lan:6767/v1/ws");
  });

  it("uses faster relay backoff without enabling retries during a one-shot probe", async () => {
    const result = await connectToDaemon(
      { id: "account", type: "accountRelay", hostId: "remote", center: "https://center.test/api" },
      undefined,
      probe.deps,
    );
    expect(probe.createdConfigs()[0]?.reconnect).toEqual({
      enabled: false,
      baseDelayMs: 500,
      maxDelayMs: 30_000,
    });
    expect(probe.createdConfigs()[0]?.connectTimeoutMs).toBe(45_000);
    await result.client.close();
  });

  it("encodes the local socket target into the client config", async () => {
    const result = await connectToDaemon(
      {
        id: "socket:/tmp/ait.sock",
        type: "directSocket",
        path: "/tmp/ait.sock",
      },
      undefined,
      probe.deps,
    );
    await result.client.close();

    expect(probe.createdConfigs()[0]?.url).toBe("ait+desktop://socket?path=%2Ftmp%2Fait.sock");
  });

  it("uses the desktop transport for Remote SSH connections", async () => {
    const transportFactory = vi.fn();
    const result = await connectToDaemon(
      {
        id: "ssh:deploy%40example.com:2222:%2Fkeys%2Fpaseo",
        type: "remoteSsh",
        host: "deploy@example.com",
        sshPort: 2222,
        daemonPort: 7777,
        password: "t".repeat(32),
      },
      undefined,
      {
        ...probe.deps,
        createDesktopTransportFactory: () => transportFactory,
      },
    );
    await result.client.close();

    expect(probe.createdConfigs()[0]).toMatchObject({
      url: "ait+desktop://ssh?host=deploy%40example.com",
      password: "t".repeat(32),
      transportFactory: expect.any(Function),
    });
  });

  it("passes direct TCP connection passwords into the client config", async () => {
    const result = await connectToDaemon(
      {
        id: "direct:lan:6767",
        type: "directTcp",
        endpoint: "lan:6767",
        password: "shared-secret",
      },
      undefined,
      probe.deps,
    );
    await result.client.close();

    expect(probe.createdConfigs()[0]?.password).toBe("shared-secret");
  });

  it("passes performance tracing into the connected client", async () => {
    const trace = {
      isEnabled: () => true,
      beginSection: vi.fn(),
      endSection: vi.fn(),
    };
    const result = await connectToDaemon(
      {
        id: "direct:lan:6767",
        type: "directTcp",
        endpoint: "lan:6767",
      },
      { trace },
      probe.deps,
    );
    await result.client.close();

    expect(probe.createdConfigs()[0]?.trace).toBe(trace);
  });

  it("surfaces auth rejection as an incorrect password", async () => {
    probe.failNextConnection(
      new Error("Transport closed (code 4001)"),
      "Transport closed (code 4001)",
    );

    await expect(
      connectToDaemon(
        {
          id: "direct:lan:6767",
          type: "directTcp",
          endpoint: "lan:6767",
          password: "wrong-secret",
        },
        undefined,
        probe.deps,
      ),
    ).rejects.toMatchObject({
      message: "Incorrect password",
    });
  });

  it("keeps generic transport failures generic when a password was supplied", async () => {
    probe.failNextConnection(new Error("Transport error"), "Transport error");

    await expect(
      connectToDaemon(
        {
          id: "direct:lan:6767",
          type: "directTcp",
          endpoint: "lan:6767",
          password: "shared-secret",
        },
        undefined,
        probe.deps,
      ),
    ).rejects.toMatchObject({
      message: "Transport error",
    });
  });
});
