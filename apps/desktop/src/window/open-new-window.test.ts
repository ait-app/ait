import { describe, expect, it } from "vitest";
import { buildHostWorkspaceRoute } from "../../../mobile/src/utils/host-routes";
import { resolveNewWindowOptions } from "./open-new-window.js";

describe("new desktop window targets", () => {
  it("preserves empty-window and project-path launches", () => {
    expect(resolveNewWindowOptions(undefined)).toEqual({
      initialRoute: null,
      pendingProjectPath: null,
    });
    expect(resolveNewWindowOptions({ pendingOpenProjectPath: "/repo/project" })).toEqual({
      initialRoute: null,
      pendingProjectPath: "/repo/project",
    });
  });

  it.each([
    { serverId: "remote-host", workspaceId: "workspace-42" },
    { serverId: "host/with?query", workspaceId: "/remote/有 空格/worktree" },
    { serverId: " shared:host ", workspaceId: " branch/feature#one " },
    { serverId: "shared/host", workspaceId: "b64_existing-id" },
  ])("matches renderer routing for $workspaceId on $serverId", (workspace) => {
    expect(resolveNewWindowOptions({ workspace })).toEqual({
      initialRoute: buildHostWorkspaceRoute(workspace.serverId, workspace.workspaceId),
      pendingProjectPath: null,
    });
  });

  it.each([
    { workspace: { serverId: "", workspaceId: "workspace" } },
    { workspace: { serverId: "host", workspaceId: " " } },
    { workspace: { serverId: 42, workspaceId: "workspace" } },
    { workspace: { serverId: "host", workspaceId: ".." } },
    { workspace: { serverId: ".", workspaceId: "workspace" } },
    { workspace: { serverId: "host", workspaceId: "workspace", route: "https://example.com" } },
    { workspace: { serverId: "host", workspaceId: "workspace" }, pendingOpenProjectPath: "/repo" },
    { initialRoute: "https://example.com" },
    "https://example.com",
  ])("rejects malformed or ambiguous launch options %j", (input) => {
    expect(() => resolveNewWindowOptions(input)).toThrow();
  });
});
