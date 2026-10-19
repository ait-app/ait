import type { ActiveConnection } from "@/runtime/host-runtime";
import type { WorkspaceScriptPayload } from "@ait/protocol/messages";
import { describe, expect, it } from "vitest";
import { resolveWorkspaceScriptLink } from "./workspace-script-links";

const runningService: WorkspaceScriptPayload = {
  scriptName: "web",
  type: "service",
  hostname: "web--feature--paseo.localhost",
  port: 3000,
  lifecycle: "running",
  health: "healthy",
  exitCode: null,
  terminalId: null,
};

function resolveLink(
  activeConnection: ActiveConnection | null,
  script: WorkspaceScriptPayload = runningService,
) {
  return resolveWorkspaceScriptLink({ script, activeConnection });
}

describe("resolveWorkspaceScriptLink", () => {
  it("uses the daemon host and service port over a direct network connection", () => {
    expect(
      resolveLink({
        type: "directTcp",
        endpoint: "mac-mini.tail123.ts.net:6767",
        display: "mac-mini.tail123.ts.net:6767",
      }),
    ).toEqual({
      label: "mac-mini.tail123.ts.net:3000",
      url: "http://mac-mini.tail123.ts.net:3000",
    });
  });

  it("uses the remote loopback address for the online relay browser tunnel", () => {
    expect(
      resolveLink({ type: "accountRelay", endpoint: "remote-host", display: "Online relay" }),
    ).toEqual({ label: "localhost:3000", url: "http://localhost:3000" });
  });

  it.each(["localhost:6767", "127.0.0.1:6767", "[::1]:6767"])(
    "uses localhost for a local daemon at %s",
    (endpoint) => {
      expect(resolveLink({ type: "directTcp", endpoint, display: endpoint })).toEqual({
        label: "localhost:3000",
        url: "http://localhost:3000",
      });
    },
  );

  it("preserves IPv6 brackets in a remote service URL", () => {
    const endpoint = "[2001:db8::1]:6767";
    expect(resolveLink({ type: "directTcp", endpoint, display: endpoint })).toEqual({
      label: "[2001:db8::1]:3000",
      url: "http://[2001:db8::1]:3000",
    });
  });

  it("uses localhost for a local daemon socket", () => {
    expect(
      resolveLink({ type: "directSocket", endpoint: "/tmp/ait.sock", display: "socket" }),
    ).toEqual({ label: "localhost:3000", url: "http://localhost:3000" });
  });

  it("has no route for stopped services, plain scripts or services without a port", () => {
    expect(resolveLink(null, { ...runningService, lifecycle: "stopped" })).toBeNull();
    expect(resolveLink(null, { ...runningService, type: "script" })).toBeNull();
    expect(resolveLink(null, { ...runningService, port: null })).toBeNull();
  });
});
