import { describe, expect, it } from "vitest";
import {
  buildWorkspaceCreationRequestKey,
  canReuseCreatedWorkspace,
  type WorkspaceCreationRequestIdentity,
} from "./creation-request-key";

const base: WorkspaceCreationRequestIdentity = {
  serverId: "srv_1",
  projectId: "prj_1",
  sourceDirectory: "/repo",
  isolation: "worktree",
  selectedItem: {
    kind: "branch",
    name: "main",
    refName: "refs/heads/main",
    accessibilityLabel: "main, local branch",
  },
};

describe("buildWorkspaceCreationRequestKey", () => {
  it("is stable for the same request", () => {
    expect(buildWorkspaceCreationRequestKey(base)).toBe(
      buildWorkspaceCreationRequestKey({ ...base }),
    );
  });

  it.each<[string, Partial<WorkspaceCreationRequestIdentity>]>([
    ["host", { serverId: "srv_2" }],
    ["project", { projectId: "prj_2" }],
    ["source directory", { sourceDirectory: "/other" }],
    ["isolation", { isolation: "local" }],
    ["sandbox", { isolation: "sandboxed-worktree" }],
    [
      "base branch",
      {
        selectedItem: {
          kind: "branch",
          name: "dev",
          refName: "refs/heads/dev",
          accessibilityLabel: "dev, local branch",
        },
      },
    ],
    ["default base", { selectedItem: null }],
  ])("changes when the %s changes", (_label, change) => {
    expect(buildWorkspaceCreationRequestKey({ ...base, ...change })).not.toBe(
      buildWorkspaceCreationRequestKey(base),
    );
  });

  it("ignores the base selection for local workspaces", () => {
    const local = { ...base, isolation: "local" as const };
    expect(buildWorkspaceCreationRequestKey({ ...local, selectedItem: null })).toBe(
      buildWorkspaceCreationRequestKey(local),
    );
  });
});

describe("canReuseCreatedWorkspace", () => {
  const requestKey = buildWorkspaceCreationRequestKey(base);

  it("reuses the workspace for a matching request without an agent", () => {
    expect(
      canReuseCreatedWorkspace({ cachedRequestKey: requestKey, requestKey, createsAgent: false }),
    ).toBe(true);
  });

  it("creates again when the request changed", () => {
    expect(
      canReuseCreatedWorkspace({ cachedRequestKey: "other", requestKey, createsAgent: false }),
    ).toBe(false);
  });

  it("creates again when the request needs an agent created with the workspace", () => {
    expect(
      canReuseCreatedWorkspace({ cachedRequestKey: requestKey, requestKey, createsAgent: true }),
    ).toBe(false);
  });
});
