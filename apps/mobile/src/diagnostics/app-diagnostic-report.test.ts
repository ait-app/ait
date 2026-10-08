import { defaultHostAppearance } from "@/hosts/appearance";
import type { HostRuntimeSnapshot } from "@/runtime/host-runtime";
import type { HostProfile } from "@/types/host-connection";
import { describe, expect, test } from "vitest";
import {
  formatHostRuntimeSection,
  formatServerInfoSection,
  redactAppDiagnosticReport,
} from "./app-diagnostic-report";

function makeHost(): HostProfile {
  return {
    serverId: "srv-secret",
    label: "Secret host",
    appearance: defaultHostAppearance(),
    lifecycle: {},
    preferredConnectionId: "direct:secret.example.test:6767",
    createdAt: "2026-06-25T00:00:00.000Z",
    updatedAt: "2026-06-25T00:00:00.000Z",
    connections: [
      {
        id: "ssh:deploy%40private-host::",
        type: "remoteSsh",
        host: "deploy@private-host",
        password: "ssh-server-token",
      },
      {
        id: "direct:secret.example.test:6767",
        type: "directTcp",
        endpoint: "secret.example.test:6767",
        useTls: true,
        password: "tcp-password",
      },
      {
        id: "relay:relay.secret.test:443",
        type: "directTcp",
        endpoint: "relay.secret.test:443",
        useTls: true,
        password: "daemon-public-key-secret",
      },
      {
        id: "socket:/tmp/paseo-secret.sock",
        type: "directSocket",
        path: "/tmp/paseo-secret.sock",
      },
      {
        id: "pipe:\\\\.\\pipe\\paseo-secret",
        type: "directPipe",
        path: "\\\\.\\pipe\\paseo-secret",
      },
    ],
  };
}

describe("app diagnostics report", () => {
  test("reports whether the connected daemon is managed by Paseo Desktop", () => {
    const report = formatServerInfoSection({
      status: "server_info",
      serverId: "srv-desktop-managed",
      hostname: "desktop-host.local",
      version: "0.1.108",
      desktopManaged: true,
    });

    expect(report).toContain("Desktop managed: yes");
  });

  test("formats connection rows without raw connection details", () => {
    const host = makeHost();
    const snapshot: HostRuntimeSnapshot = {
      serverId: host.serverId,
      activeConnectionId: "relay:relay.secret.test:443",
      activeConnection: {
        type: "directTcp",
        endpoint: "relay.secret.test:443",
        display: "remote",
      },
      connectionStatus: "online",
      client: null,
      lastError: null,
      lastOnlineAt: "2026-06-25T00:00:00.000Z",
      agentDirectoryStatus: "ready",
      agentDirectoryError: null,
      hasEverLoadedAgentDirectory: true,
      probeByConnectionId: new Map([
        ["direct:secret.example.test:6767", { status: "available", latencyMs: 42 }],
        ["relay:relay.secret.test:443", { status: "available", latencyMs: 8 }],
      ]),
      clientGeneration: 1,
      connectionEpoch: 1,
    };

    const report = formatHostRuntimeSection({ host, snapshot });

    expect(report).toContain("direct TCP");
    expect(report).toContain("remote SSH");
    expect(report).not.toContain("ssh-server-token");
    expect(report).not.toContain("deploy@private-host");
    expect(report).toContain("local socket");
    expect(report).toContain("local pipe");
    expect(report).not.toContain("secret.example.test");
    expect(report).not.toContain("relay.secret.test");
    expect(report).not.toContain("daemon-public-key-secret");
    expect(report).not.toContain("/tmp/paseo-secret.sock");
    expect(report).not.toContain("tcp-password");
  });

  test("redacts saved connection secrets from collected daemon and desktop text", () => {
    const host = makeHost();
    const redacted = redactAppDiagnosticReport(
      [
        "Desktop app log tail",
        "unstructured ssh-server-token from deploy@private-host",
        "secret.example.test:6767",
        "relay.secret.test:443",
        "daemon-public-key-secret",
        "/tmp/paseo-secret.sock",
        "\\\\.\\pipe\\paseo-secret",
        "password=tcp-password",
        "paseo://pairing-secret",
        "ait://ait-pairing-secret",
      ].join("\n"),
      [host],
    );

    expect(redacted).not.toContain("secret.example.test");
    expect(redacted).not.toContain("relay.secret.test");
    expect(redacted).not.toContain("daemon-public-key-secret");
    expect(redacted).not.toContain("/tmp/paseo-secret.sock");
    expect(redacted).not.toContain("\\\\.\\pipe\\paseo-secret");
    expect(redacted).not.toContain("tcp-password");
    expect(redacted).not.toContain("ssh-server-token");
    expect(redacted).not.toContain("deploy@private-host");
    expect(redacted).not.toContain("pairing-secret");
    expect(redacted).toContain("ait://[redacted]");
  });
});

test("redacts quoted native credentials and complete authorization headers", () => {
  const report = redactAppDiagnosticReport(
    'Authorization: Bearer secret-one\n{"api_key":"secret-two","refresh_token":"secret-three"}\nauthorization=Basic c2VjcmV0',
    [],
  );
  for (const secret of ["secret-one", "secret-two", "secret-three", "c2VjcmV0"]) {
    expect(report).not.toContain(secret);
  }
});
