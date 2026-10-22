/** @vitest-environment jsdom */
import React from "react";
import "@/i18n/i18next";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useHubServices } from "@/runtime/hub-service-store";
import { emptyHubDirectory } from "@/runtime/hub-services";
import { AccountDirectoryPanel } from "./account-directory-panel";
const navigate = vi.hoisted(() => vi.fn());
vi.mock("expo-router", () => ({ router: { navigate } }));
vi.mock("@/runtime/account-state", () => ({
  useAccountState: (select: (state: unknown) => unknown) =>
    select({
      hosts: [
        { host_id: "host-a", server_id: "server-a", name: "Computer A", online: true },
        { host_id: "host-b", server_id: "server-b", name: "Computer B", online: false },
      ],
    }),
}));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
describe("account directory UI", () => {
  it("keeps equal resource IDs on different Hosts separate and disables offline details", () => {
    useHubServices.setState({
      snapshot: {
        state: "online",
        linkId: "link",
        centerUrl: "https://center",
        methods: ["workspace.list.request"],
      },
      directory: {
        ...emptyHubDirectory(),
        workspace: [
          {
            hostId: "host-a",
            id: "same",
            data: { title: "Workspace A" },
            revision: 1,
            updatedAt: "now",
            online: true,
            stale: false,
          },
          {
            hostId: "host-b",
            id: "same",
            data: { title: "Workspace B" },
            revision: 1,
            updatedAt: "now",
            online: false,
            stale: true,
          },
        ],
      },
    });
    const view = render(<AccountDirectoryPanel />);
    expect(view.getByText(/Computer B · Offline/)).toBeTruthy();
    fireEvent.click(view.getByTestId("account-directory-workspace-host-b-same"));
    expect(navigate).not.toHaveBeenCalled();
    fireEvent.click(view.getByTestId("account-directory-workspace-host-a-same"));
    expect(navigate).toHaveBeenCalledOnce();
    expect(navigate.mock.calls[0][0]).toContain("server-a");
    fireEvent.change(view.getByTestId("account-directory-search"), {
      target: { value: "Workspace B" },
    });
    expect(view.queryByText("Workspace A")).toBeNull();
    expect(view.getByText("Workspace B")).toBeTruthy();
  });
});
