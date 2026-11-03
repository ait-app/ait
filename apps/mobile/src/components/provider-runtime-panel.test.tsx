// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ProviderUiState } from "@ait/protocol/messages";
import { ProviderRuntimePanel } from "./provider-runtime-panel";

const fixture = vi.hoisted(() => ({
  state: null as ProviderUiState | null,
  open: vi.fn(async () => {}),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (selector: (state: unknown) => unknown) =>
    selector({
      sessions: { server: { agents: new Map([["agent", { providerUi: fixture.state }]]) } },
    }),
}));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: fixture.open }));
vi.mock("./provider-native-actions", () => ({ ProviderNativeActions: () => null }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: () => "Open in browser" }) }));
afterEach(() => {
  cleanup();
  fixture.open.mockClear();
});

it("updates and removes native UI, preserves widget placement, and opens links on click", () => {
  fixture.state = {
    statuses: [{ key: "build", text: "Build ready" }],
    widgets: [{ key: "summary", lines: ["First", "Second"], placement: "belowEditor" }],
    title: "Native session",
    editor: null,
    link: {
      url: "https://example.com/auth",
      launchUrl: "https://example.com/launch",
      instructions: "Enter code",
    },
  };
  const above = render(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.getByText("Build ready")).toBeTruthy();
  expect(screen.queryByText(/First/)).toBeNull();
  expect(fixture.open).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Open in browser"));
  expect(fixture.open).toHaveBeenCalledExactlyOnceWith("https://example.com/launch");
  render(<ProviderRuntimePanel serverId="server" agentId="agent" placement="belowEditor" />);
  expect(screen.getByText(/First.*Second/)).toBeTruthy();
  fixture.state = { ...fixture.state, statuses: [], widgets: [], link: null, title: null };
  above.rerender(
    <ProviderRuntimePanel serverId="server" agentId="agent" placement="aboveEditor" />,
  );
  expect(screen.queryByText("Build ready")).toBeNull();
  expect(screen.queryByText("Open in browser")).toBeNull();
});

it("defers editor replacement during submit and delivers each revision once across navigation", () => {
  fixture.state = {
    statuses: [],
    widgets: [],
    title: null,
    link: null,
    editor: { id: "revision-one", text: "Native draft" },
  };
  const onEditor = vi.fn();
  const props = {
    serverId: "server",
    agentId: "agent",
    placement: "aboveEditor" as const,
    onEditor,
  };
  const mounted = render(<ProviderRuntimePanel {...props} blocked />);
  expect(onEditor).not.toHaveBeenCalled();
  mounted.rerender(<ProviderRuntimePanel {...props} />);
  expect(onEditor).toHaveBeenCalledExactlyOnceWith("Native draft");
  mounted.unmount();
  const reopened = render(<ProviderRuntimePanel {...props} />);
  expect(onEditor).toHaveBeenCalledTimes(1);
  fixture.state = { ...fixture.state, editor: { id: "revision-two", text: "" } };
  reopened.rerender(<ProviderRuntimePanel {...props} />);
  expect(onEditor).toHaveBeenLastCalledWith("");
  expect(onEditor).toHaveBeenCalledTimes(2);
});
