// @vitest-environment jsdom
import { createRef } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { View } from "react-native";
import type { ComboboxProps } from "@/components/ui/combobox";
import { defaultHostAppearance } from "@/hosts/appearance";
import { createAccountRelayHostConnection, type HostProfile } from "@/types/host-connection";
import { HostPicker } from "./host-picker";

vi.mock("@/hooks/use-is-local-daemon", () => ({ useLocalDaemonServerId: () => "local" }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeSnapshot: () => null }));
vi.mock("@/components/host-status-dot", () => ({ HostStatusDot: () => null }));
// Keep the production picker model and callbacks; replace only the overlay surface.
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({ options, value, onSelect }: ComboboxProps) => (
    <div>
      {options.map((option) => (
        <button
          key={option.id}
          aria-pressed={value === option.id}
          onClick={() => onSelect(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  ),
  ComboboxItem: () => null,
}));

const hostId = "11111111-1111-4111-8111-111111111111";
const shareIds = {
  workspace: "22222222-2222-4222-8222-222222222222",
  project: "33333333-3333-4333-8333-333333333333",
  node: "44444444-4444-4444-8444-444444444444",
} as const;

function sharedHost(
  shareName: keyof typeof shareIds,
  scope: "node" | "project" | "workspace",
  physicalHostId = hostId,
  center = "https://center.test/api",
): HostProfile {
  const shareId = shareIds[shareName];
  const connection = createAccountRelayHostConnection({
    hostId: physicalHostId,
    center,
    shareId,
    nodeId: "node",
    shareResourceKind: scope,
  });
  return {
    serverId: `shared:${shareId}`,
    label: "Owner · Workstation",
    appearance: defaultHostAppearance(),
    lifecycle: {},
    connections: [connection],
    preferredConnectionId: connection.id,
    createdAt: "2026-11-01T00:00:00Z",
    updatedAt: "2026-11-01T00:00:00Z",
  };
}

function picker(hosts: HostProfile[], onSelect = vi.fn(), value = "") {
  return (
    <HostPicker
      hosts={hosts}
      value={value}
      onSelect={onSelect}
      open
      onOpenChange={() => {}}
      anchorRef={createRef<View>()}
      children={null}
    />
  );
}

afterEach(cleanup);

describe("Hosts list shared grants", () => {
  it("shows one Host for workspace and project grants without removing either connection", () => {
    const workspace = sharedHost("workspace", "workspace");
    const project = sharedHost("project", "project");
    const onSelect = vi.fn();
    const hosts = [workspace, project];
    const originalHosts = structuredClone(hosts);
    const view = render(picker(hosts, onSelect, workspace.serverId));
    const rows = view.getAllByRole("button", { name: "Owner · Workstation" });
    expect(rows).toHaveLength(1);
    expect(rows[0].getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(rows[0]);
    expect(onSelect).toHaveBeenLastCalledWith(project.serverId);
    expect(hosts).toEqual(originalHosts);
    expect(workspace.connections).toHaveLength(1);
    expect(project.connections).toHaveLength(1);

    // Revoking the representative grant leaves the other grant usable.
    view.rerender(picker([workspace], onSelect, workspace.serverId));
    fireEvent.click(view.getByRole("button", { name: "Owner · Workstation" }));
    expect(onSelect).toHaveBeenLastCalledWith(workspace.serverId);
  });

  it("does not combine identically named Hosts or Hosts from different centers", () => {
    const view = render(
      picker([
        sharedHost("workspace", "workspace"),
        sharedHost("project", "project", "55555555-5555-4555-8555-555555555555"),
        sharedHost("node", "project", hostId, "https://other.test/api"),
      ]),
    );
    expect(view.getAllByRole("button", { name: "Owner · Workstation" })).toHaveLength(3);
  });

  it("prefers a node grant for Host settings regardless of share order", () => {
    const workspace = sharedHost("workspace", "workspace");
    const project = sharedHost("project", "project");
    const node = sharedHost("node", "node");
    const onSelect = vi.fn();
    const view = render(picker([workspace, project, node], onSelect));
    fireEvent.click(view.getByRole("button", { name: "Owner · Workstation" }));
    expect(onSelect).toHaveBeenLastCalledWith(node.serverId);
    view.rerender(picker([node, project, workspace], onSelect));
    fireEvent.click(view.getByRole("button", { name: "Owner · Workstation" }));
    expect(onSelect).toHaveBeenLastCalledWith(node.serverId);
  });

  it("keeps owned Hosts separate and places the local Host first", () => {
    const workspace = sharedHost("workspace", "workspace");
    const project = sharedHost("project", "project");
    const local: HostProfile = {
      ...workspace,
      serverId: "local",
      connections: [{ id: "direct", type: "directTcp", endpoint: "localhost:1234" }],
      preferredConnectionId: "direct",
    };
    const otherOwned = { ...local, serverId: "owned-remote" };
    const onSelect = vi.fn();
    const view = render(picker([workspace, project, otherOwned, local], onSelect));
    const rows = view.getAllByRole("button", { name: "Owner · Workstation" });
    expect(rows).toHaveLength(3);
    fireEvent.click(rows[0]);
    expect(onSelect).toHaveBeenLastCalledWith("local");
  });

  it("groups Host rows by default for every picker caller", () => {
    const view = render(
      picker([sharedHost("workspace", "workspace"), sharedHost("project", "project")], vi.fn(), ""),
    );
    expect(view.getAllByRole("button", { name: "Owner · Workstation" })).toHaveLength(1);
  });
});
