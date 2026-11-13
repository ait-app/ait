import { router } from "expo-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { returnFromSettings } from "./settings-navigation";

// Self-contained double: the global setup mock predates dismissTo, and
// extending it via importMock from inside a factory would recurse.
vi.mock("expo-router", () => ({
  router: {
    push: vi.fn(),
    replace: vi.fn(),
    dismissTo: vi.fn(),
    back: vi.fn(),
    navigate: vi.fn(),
  },
}));

vi.mock("@/stores/navigation-active-workspace-store", () => ({
  navigateToLastWorkspace: vi.fn(() => false),
}));

describe("returnFromSettings", () => {
  beforeEach(() => {
    vi.mocked(router.dismissTo).mockClear();
    vi.mocked(router.replace).mockClear();
  });

  it("returns from a nested profile editor to the host's agents section", () => {
    returnFromSettings({ kind: "agentProfile", serverId: "host a", profileId: "p1" });
    expect(router.dismissTo).toHaveBeenCalledWith("/settings/hosts/host%20a/agents");
  });

  it("returns from the create form to the same agents section", () => {
    returnFromSettings({ kind: "agentProfile", serverId: "h1", profileId: null });
    expect(router.dismissTo).toHaveBeenCalledWith("/settings/hosts/h1/agents");
  });

  it("returns from a project detail to the host's project list", () => {
    returnFromSettings({ kind: "project", serverId: "h1", projectId: "proj" });
    expect(router.dismissTo).toHaveBeenCalledWith("/settings/hosts/h1/projects");
  });

  it("returns from a host section to the settings root", () => {
    returnFromSettings({ kind: "host", serverId: "h1", section: "agents" });
    expect(router.dismissTo).toHaveBeenCalledWith("/settings");
  });
});
