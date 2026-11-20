// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { View } from "react-native";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  SidebarDisplayPreferencesFallbackRow,
  SidebarDisplayPreferencesMenuHost,
  SidebarDisplayPreferencesTrigger,
} from "./menu";

const mocks = vi.hoisted(() => ({ hosts: [] as never[], surfaceMounts: 0 }));

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));
vi.mock("@/runtime/host-runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime/host-runtime")>()),
  useHosts: () => mocks.hosts,
}));
vi.mock("@/components/sidebar/sidebar-model", () => ({
  useSidebarModel: () => {
    mocks.surfaceMounts += 1;
    return { allProjects: [], resolvedProjectFilters: [] };
  },
}));
vi.mock("@/workspace-labels/manager-modal", () => ({ WorkspaceLabelManagerModal: () => null }));

afterEach(() => {
  cleanup();
  queryClient.clear();
  mocks.surfaceMounts = 0;
});

function trigger(id: string): HTMLElement {
  return screen.getByTestId(`sidebar-display-preferences-menu-${id}`);
}

function menu(): Promise<HTMLElement> {
  return screen.findByTestId("sidebar-display-preferences-content");
}

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

function Sidebar({ rows }: { rows: string[] }) {
  return (
    <QueryClientProvider client={queryClient}>
      <SidebarDisplayPreferencesMenuHost>
        {rows.map((row) => (
          <View key={row} testID={`row-${row}`}>
            <SidebarDisplayPreferencesTrigger triggerId={row} />
          </View>
        ))}
        {rows.length === 0 ? <SidebarDisplayPreferencesFallbackRow /> : null}
      </SidebarDisplayPreferencesMenuHost>
    </QueryClientProvider>
  );
}

describe("sidebar display menu", () => {
  it("reads nothing for its contents until a trigger opens it", async () => {
    render(<Sidebar rows={["a", "b"]} />);
    expect(mocks.surfaceMounts).toBe(0);
    expect(screen.queryByTestId("sidebar-display-preferences-content")).toBeNull();

    fireEvent.click(trigger("b"));
    await menu();
    expect(trigger("b").getAttribute("aria-expanded")).toBe("true");
    expect(trigger("a").getAttribute("aria-expanded")).toBe("false");
  });

  it("stays open when the row it was opened from leaves the list", async () => {
    // Filtering to another project, or switching the grouping, removes that row mid-selection.
    const view = render(<Sidebar rows={["a", "b"]} />);
    fireEvent.click(trigger("a"));
    await menu();

    view.rerender(<Sidebar rows={["b"]} />);
    expect(screen.queryByTestId("sidebar-display-preferences-menu-a")).toBeNull();
    expect(screen.getByTestId("sidebar-display-preferences-content")).toBeTruthy();

    view.rerender(<Sidebar rows={[]} />);
    expect(screen.getByTestId("sidebar-display-preferences-content")).toBeTruthy();
  });

  it("opens from the fallback row when no row is left to carry a trigger", async () => {
    render(<Sidebar rows={[]} />);
    fireEvent.click(trigger("fallback"));
    await menu();
  });
});
