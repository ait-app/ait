// @vitest-environment jsdom
import "fake-indexeddb/auto";
import type { ComponentProps, ReactNode } from "react";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { ShareGroup, ShareSetting } from "@ait/protocol/messages";
import type { ComboboxProps } from "@/components/ui/combobox";
import type { Switch } from "@/components/ui/switch";
import { defaultHostAppearance } from "@/hosts/appearance";
import { i18n } from "@/i18n/i18next";
import type { HostProfile } from "@/types/host-connection";
import { ShareManagerSheet, type ShareResourceTarget } from "./share-manager-sheet";

type ShareClient = Pick<DaemonClient, "listShares" | "setShare" | "removeShare">;
const state = vi.hoisted(() => ({
  hosts: [] as HostProfile[],
  clients: new Map<string, ShareClient>(),
  activeServerId: "remote" as string | null,
  localServerId: "local" as string | null,
}));

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));
vi.mock("@/runtime/host-runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime/host-runtime")>()),
  useHosts: () => state.hosts,
  useHostRuntimeClient: (serverId: string) => state.clients.get(serverId) ?? null,
}));
vi.mock("@/hooks/use-is-local-daemon", () => ({
  useLocalDaemonServerId: () => state.localServerId,
}));
vi.mock("@/stores/navigation-active-workspace-store", () => ({
  useActiveWorkspaceSelection: () =>
    state.activeServerId ? { serverId: state.activeServerId, workspaceId: "workspace" } : null,
}));
vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({ visible, children }: { visible: boolean; children: ReactNode }) =>
    visible ? children : null,
}));
vi.mock("@/components/ui/switch", () => ({
  Switch: ({
    value,
    onValueChange,
    disabled,
    accessibilityLabel,
  }: ComponentProps<typeof Switch>) => (
    <button
      role="switch"
      aria-label={accessibilityLabel}
      aria-checked={value}
      disabled={disabled}
      onClick={() => onValueChange?.(!value)}
    />
  ),
}));
// Exercise the production HostPicker with a lightweight overlay surface.
vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({ options, value, onSelect, open }: ComboboxProps) =>
    open ? (
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
    ) : null,
  ComboboxItem: () => null,
}));

const group: ShareGroup = {
  groupId: "11111111-1111-4111-8111-111111111111",
  name: "Team",
  myRole: "admin",
  status: "active",
};
const target: ShareResourceTarget = {
  serverId: "local",
  resourceKind: "project",
  resourceKey: "local-project",
  displayName: "Project",
  projectHosts: [
    { serverId: "local", projectId: "local-project" },
    { serverId: "remote", projectId: "remote-project" },
    { serverId: "guest", projectId: "guest-project" },
  ],
};

function share(
  resourceKey: string,
  resourceKind: ShareSetting["resourceKind"] = "project",
): ShareSetting {
  return {
    resourceKind,
    resourceKey,
    groupId: group.groupId,
    permission: "operate",
    audience: "group",
    recipients: [],
    settingId: "22222222-2222-4222-8222-222222222222",
    state: "accepted",
    missingRecipients: [],
  };
}

function client(shares: ShareSetting[] = []) {
  return {
    listShares: vi
      .fn<ShareClient["listShares"]>()
      .mockResolvedValue({ requestId: "list", groups: [group], shares }),
    setShare: vi.fn<ShareClient["setShare"]>().mockImplementation(async (input) => ({
      requestId: "set",
      share: share(input.resourceKey, input.resourceKind),
    })),
    removeShare: vi
      .fn<ShareClient["removeShare"]>()
      .mockImplementation(async (input) => ({ requestId: "remove", ...input, removed: true })),
  };
}

function host(serverId: string): HostProfile {
  return {
    serverId,
    label: `Host ${serverId}`,
    appearance: defaultHostAppearance(),
    lifecycle: {},
    connections: [{ id: "direct", type: "directTcp", endpoint: "localhost:1234" }],
    preferredConnectionId: "direct",
    createdAt: "2026-11-01T00:00:00Z",
    updatedAt: "2026-11-01T00:00:00Z",
  };
}

function sheet(resource = target, visible = true) {
  return <ShareManagerSheet visible={visible} target={resource} onClose={() => {}} />;
}

function chooseHost(view: ReturnType<typeof render>, serverId: string) {
  fireEvent.click(view.getByTestId("share-host-picker"));
  fireEvent.click(view.getByRole("button", { name: `Host ${serverId}` }));
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  const guest = host("guest");
  guest.connections = [
    {
      id: "guest-relay",
      type: "accountRelay",
      hostId: "33333333-3333-4333-8333-333333333333",
      center: "https://center.test/api",
      nodeId: "node",
      shareId: "44444444-4444-4444-8444-444444444444",
      shareResourceKind: "project",
    },
  ];
  state.hosts = [host("local"), host("remote"), host("other"), guest];
  state.clients = new Map([
    ["local", client()],
    ["remote", client()],
    ["guest", client()],
  ]);
  state.activeServerId = "remote";
  state.localServerId = "local";
});
afterEach(cleanup);

describe("project sharing host selection", () => {
  it("defaults to the current Host and only offers owned Hosts containing the project", async () => {
    const view = render(sheet());
    await view.findByRole("switch", { name: "Team" });
    expect(view.getByTestId("share-host-picker").textContent).toBe("Host remote");
    expect(state.clients.get("remote")?.listShares).toHaveBeenCalledOnce();
    expect(state.clients.get("local")?.listShares).not.toHaveBeenCalled();
    fireEvent.click(view.getByTestId("share-host-picker"));
    expect(view.getByRole("button", { name: "Host remote" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    expect(view.getByRole("button", { name: "Host local" })).toBeTruthy();
    expect(view.queryByRole("button", { name: "Host guest" })).toBeNull();
    expect(view.queryByRole("button", { name: "Host other" })).toBeNull();
  });

  it.each([null, "other", "guest"])(
    "defaults to the local Host when %s has no eligible placement",
    async (activeServerId) => {
      state.activeServerId = activeServerId;
      const view = render(sheet());
      await view.findByRole("switch", { name: "Team" });
      expect(view.getByTestId("share-host-picker").textContent).toBe("Host local");
    },
  );

  it("falls back to the project's Host when neither current nor local Host contains it", async () => {
    state.activeServerId = "other";
    state.localServerId = "other";
    const view = render(sheet());
    await view.findByRole("switch", { name: "Team" });
    expect(view.getByTestId("share-host-picker").textContent).toBe("Host local");
  });

  it("loads and changes each Host's own project sharing independently", async () => {
    const remote = client([share("local-project"), share("remote-project", "workspace")]);
    const local = client([share("local-project")]);
    state.clients.set("remote", remote);
    state.clients.set("local", local);
    const view = render(sheet());
    let toggle = await view.findByRole("switch", { name: "Team" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
    expect(remote.setShare).toHaveBeenCalledWith({
      resourceKind: "project",
      resourceKey: "remote-project",
      groupId: group.groupId,
      displayName: "Project",
      projectName: undefined,
    });
    expect(local.setShare).not.toHaveBeenCalled();

    chooseHost(view, "local");
    toggle = await view.findByRole("switch", { name: "Team" });
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
    expect(local.removeShare).toHaveBeenCalledWith({
      resourceKind: "project",
      resourceKey: "local-project",
      groupId: group.groupId,
    });
    expect(remote.removeShare).not.toHaveBeenCalled();
  });

  it.each([false, true])("ignores an old Host's late response (failure: %s)", async (fails) => {
    const pending = Promise.withResolvers<Awaited<ReturnType<ShareClient["listShares"]>>>();
    const remote = client();
    remote.listShares.mockReturnValue(pending.promise);
    state.clients.set("remote", remote);
    const view = render(sheet());
    chooseHost(view, "local");
    const toggle = await view.findByRole("switch", { name: "Team" });
    await act(async () => {
      if (fails) pending.reject(new Error("old request failed"));
      else
        pending.resolve({
          requestId: "list-old",
          groups: [{ ...group, name: "Old Host Team" }],
          shares: [share("remote-project")],
        });
    });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(view.queryByText("Old Host Team")).toBeNull();
    expect(view.queryByText(i18n.t("sharing.loadError"))).toBeNull();
  });

  it("keeps the selected Host fixed while a sharing update is pending", async () => {
    const pending = Promise.withResolvers<Awaited<ReturnType<ShareClient["setShare"]>>>();
    const remote = client();
    remote.setShare.mockReturnValue(pending.promise);
    state.clients.set("remote", remote);
    const view = render(sheet());
    fireEvent.click(await view.findByRole("switch", { name: "Team" }));
    expect(view.getByTestId("share-host-picker").getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(view.getByTestId("share-host-picker"));
    expect(view.queryByRole("button", { name: "Host local" })).toBeNull();
    await act(async () => pending.resolve({ requestId: "set", share: share("remote-project") }));
    chooseHost(view, "local");
    await view.findByRole("switch", { name: "Team" });
    expect(view.getByTestId("share-host-picker").textContent).toBe("Host local");
  });

  it("resets to the current Host when reopened", async () => {
    const view = render(sheet());
    await view.findByRole("switch", { name: "Team" });
    chooseHost(view, "local");
    await view.findByRole("switch", { name: "Team" });
    view.rerender(sheet(target, false));
    view.rerender(sheet());
    await view.findByRole("switch", { name: "Team" });
    expect(view.getByTestId("share-host-picker").textContent).toBe("Host remote");
  });

  it("shows an unavailable Host error and lets the user choose another Host", async () => {
    state.clients.delete("remote");
    const view = render(sheet());
    expect(view.getByText(i18n.t("sharing.hostUnavailable"))).toBeTruthy();
    expect(view.queryByRole("switch")).toBeNull();
    chooseHost(view, "local");
    await view.findByRole("switch", { name: "Team" });
    expect(view.queryByText(i18n.t("sharing.hostUnavailable"))).toBeNull();
  });

  it.each(["workspace", "node"] as const)(
    "keeps %s sharing on its original Host",
    async (resourceKind) => {
      const view = render(sheet({ ...target, resourceKind }));
      await view.findByRole("switch", { name: "Team" });
      expect(view.queryByTestId("share-host-picker")).toBeNull();
      expect(state.clients.get("local")?.listShares).toHaveBeenCalledOnce();
      expect(state.clients.get("remote")?.listShares).not.toHaveBeenCalled();
    },
  );

  it("does not allow a received project share to be shared again", () => {
    const view = render(
      sheet({
        ...target,
        serverId: "guest",
        resourceKey: "guest-project",
        projectHosts: [{ serverId: "guest", projectId: "guest-project" }],
      }),
    );
    expect(view.getByText(i18n.t("sharing.ownerOnly"))).toBeTruthy();
    expect(view.queryByTestId("share-host-picker")).toBeNull();
    expect(state.clients.get("guest")?.listShares).not.toHaveBeenCalled();
  });
});
