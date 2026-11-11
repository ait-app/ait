/**
 * @vitest-environment jsdom
 */
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { FetchProviderSessionPreviewEntry } from "@ait/client/internal/daemon-client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionPreviewSheet } from "@/components/session-preview-sheet";
import { queryClient as appQueryClient } from "@/data/query-client";

const { theme, hostSnapshot } = vi.hoisted(() => ({
  hostSnapshot: { clientGeneration: 1, connectionEpoch: 1 },
  theme: {
    spacing: { 1: 4, 1.5: 6, 2: 8, 2.5: 10, 3: 12, 4: 16, 6: 24, 8: 32 },
    borderWidth: { 1: 1 },
    borderRadius: { md: 6, lg: 8, full: 9999 },
    fontSize: { xs: 11, sm: 13, base: 15 },
    fontWeight: { normal: "400", medium: "500", semibold: "600" },
    fontFamily: { mono: "monospace" },
    iconSize: { sm: 14, md: 16, lg: 24 },
    shadow: {
      sm: {
        shadowColor: "#000",
        shadowOffset: { width: 0, height: 2 },
        shadowRadius: 4,
        elevation: 2,
      },
      md: {
        shadowColor: "#000",
        shadowOffset: { width: 0, height: 4 },
        shadowRadius: 8,
        elevation: 8,
      },
      lg: {
        shadowColor: "#000",
        shadowOffset: { width: 0, height: 12 },
        shadowRadius: 24,
        elevation: 8,
      },
    },
    opacity: { 50: 0.5 },
    colors: {
      foreground: "#fff",
      foregroundMuted: "#aaa",
      surface0: "#000",
      surface1: "#111",
      surface2: "#222",
      surface3: "#333",
      border: "#444",
      borderAccent: "#555",
      interactionHighlight: "rgba(255,255,255,0.08)",
      statusDanger: "#f87171",
      palette: { red: { 300: "#f87171" } },
    },
  },
}));

vi.hoisted(() => {
  (globalThis as unknown as { __DEV__: boolean }).__DEV__ = false;
});

vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: unknown) => (typeof factory === "function" ? factory(theme) : factory),
  },
  useUnistyles: () => ({ theme }),
}));

vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeSnapshot: () => hostSnapshot,
}));

vi.mock("@/components/markdown/renderer", () => ({
  MarkdownRenderer: ({ text }: { text: string }) => React.createElement("span", null, text),
}));

vi.mock("@/components/adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    visible,
    children,
    footer,
    testID,
  }: {
    visible: boolean;
    children?: React.ReactNode;
    footer?: React.ReactNode;
    testID?: string;
  }) => (visible ? React.createElement("div", { "data-testid": testID }, children, footer) : null),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    onPress,
    testID,
    disabled,
  }: {
    children?: React.ReactNode;
    onPress?: () => void;
    testID?: string;
    disabled?: boolean;
  }) =>
    React.createElement("button", { "data-testid": testID, onClick: onPress, disabled }, children),
}));

vi.mock("@/components/ui/loading-spinner", () => ({
  LoadingSpinner: () => React.createElement("span", { "data-testid": "preview-loading" }),
}));

vi.mock("lucide-react-native", () => ({
  ChevronUp: () => React.createElement("span", { "data-icon": "ChevronUp" }),
  Eye: () => React.createElement("span", { "data-icon": "Eye" }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/utils/time", () => ({
  formatTimeAgo: () => "just now",
}));

const entry = {
  providerId: "deepseek-harness",
  providerLabel: "DeepSeek Harness",
  providerHandleId: "native-session",
  cwd: "/repo/paseo",
  title: "Native conversation",
  firstPromptPreview: null,
  lastPromptPreview: null,
  lastActivityAt: "2026-11-11T10:00:00.000Z",
};

function message(key: string, text: string): FetchProviderSessionPreviewEntry {
  return {
    key,
    timestamp: "2026-11-11T10:00:00.000Z",
    item: { type: "user_message", text },
  };
}

// The host returns newest-first pages whose entries are also newest-first.
const newestPage = {
  providerId: entry.providerId,
  providerHandleId: entry.providerHandleId,
  cwd: entry.cwd,
  title: "Native conversation",
  lastActivityAt: entry.lastActivityAt,
  active: false,
  entries: [message("m4", "message 4"), message("m3", "message 3")],
  olderCursor: "cursor-older",
  hasOlder: true,
};
const olderPage = {
  ...newestPage,
  entries: [message("m2", "message 2"), message("m1", "message 1")],
  olderCursor: null,
  hasOlder: false,
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  hostSnapshot.clientGeneration = 1;
  hostSnapshot.connectionEpoch = 1;
});

describe("SessionPreviewSheet pagination", () => {
  it("renders older pages before newer ones with stable keys after loading older", async () => {
    const fetchProviderSessionPreview = vi
      .fn()
      .mockResolvedValueOnce(newestPage)
      .mockResolvedValueOnce(olderPage);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      React.createElement(
        QueryClientProvider,
        { client: queryClient },
        React.createElement(SessionPreviewSheet, {
          visible: true,
          client: { fetchProviderSessionPreview },
          serverId: "server-1",
          entry,
          onClose: vi.fn(),
        }),
      ),
    );

    await waitFor(() => expect(screen.getByText("message 4")).toBeTruthy());
    expect(screen.queryByText("message 1")).toBeNull();

    fireEvent.click(screen.getByTestId("session-preview-load-older"));
    await waitFor(() => expect(screen.getByText("message 1")).toBeTruthy());

    // The accumulated window must read oldest-first: m1, m2 (older page) then m3, m4.
    const rendered = screen.getAllByText(/^message \d$/).map((node) => node.textContent);
    expect(rendered).toEqual(["message 1", "message 2", "message 3", "message 4"]);

    // Stable native keys survive pagination; the older page echoes the same cursor identity.
    expect(fetchProviderSessionPreview).toHaveBeenCalledTimes(2);
    expect(fetchProviderSessionPreview.mock.calls[1][0]).toMatchObject({
      providerId: "deepseek-harness",
      providerHandleId: "native-session",
      cwd: "/repo/paseo",
      cursor: "cursor-older",
    });
    expect(screen.getByTestId("session-preview-user-m1")).toBeTruthy();
    expect(screen.getByTestId("session-preview-user-m4")).toBeTruthy();
    expect(screen.queryByTestId("session-preview-load-older")).toBeNull();
    queryClient.clear();
  });

  it("ignores a late preview after the same Host client reconnects", async () => {
    let resolveOld!: (value: typeof newestPage) => void;
    const staleReply = new Promise<typeof newestPage>((resolve) => {
      resolveOld = resolve;
    });
    const fetchProviderSessionPreview = vi
      .fn()
      .mockReturnValueOnce(staleReply)
      .mockResolvedValueOnce({
        ...olderPage,
        entries: [message("fresh", "fresh connection")],
      });
    const client = { fetchProviderSessionPreview };
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = () =>
      React.createElement(
        QueryClientProvider,
        { client: queryClient },
        React.createElement(SessionPreviewSheet, {
          visible: true,
          client,
          serverId: "server-1",
          entry,
          onClose: vi.fn(),
        }),
      );
    try {
      const rendered = render(view());
      await waitFor(() => expect(fetchProviderSessionPreview).toHaveBeenCalledTimes(1));
      hostSnapshot.connectionEpoch = 2;
      rendered.rerender(view());
      await waitFor(() => expect(screen.getByText("fresh connection")).toBeTruthy());
      await act(async () => {
        resolveOld({
          ...newestPage,
          entries: [message("stale", "stale connection")],
        });
      });
      expect(screen.queryByText("stale connection")).toBeNull();
      expect(screen.getByText("fresh connection")).toBeTruthy();
      expect(fetchProviderSessionPreview).toHaveBeenCalledTimes(2);
    } finally {
      queryClient.clear();
    }
  });

  it("reopens with a fresh first page under production cache defaults", async () => {
    const fetchProviderSessionPreview = vi
      .fn()
      .mockResolvedValueOnce(newestPage)
      .mockResolvedValueOnce(olderPage)
      .mockResolvedValueOnce({
        ...newestPage,
        entries: [message("m5", "message 5")],
        olderCursor: "fresh-cut-older",
      });
    const client = { fetchProviderSessionPreview };
    const queryClient = new QueryClient({ defaultOptions: appQueryClient.getDefaultOptions() });
    const view = (visible: boolean) =>
      React.createElement(
        QueryClientProvider,
        { client: queryClient },
        React.createElement(SessionPreviewSheet, {
          visible,
          client,
          serverId: "server-1",
          entry,
          onClose: vi.fn(),
        }),
      );
    try {
      const rendered = render(view(true));
      await waitFor(() => expect(screen.getByText("message 4")).toBeTruthy());
      fireEvent.click(screen.getByTestId("session-preview-load-older"));
      await waitFor(() => expect(screen.getByText("message 1")).toBeTruthy());
      rendered.rerender(view(false));
      expect(screen.queryByTestId("session-preview-sheet")).toBeNull();
      rendered.rerender(view(true));
      await waitFor(() => expect(screen.getByText("message 5")).toBeTruthy());
      expect(screen.queryByText("message 1")).toBeNull();
      expect(screen.queryByText("message 4")).toBeNull();
      expect(fetchProviderSessionPreview).toHaveBeenCalledTimes(3);
      expect(fetchProviderSessionPreview.mock.calls[2][0]).not.toHaveProperty("cursor");
    } finally {
      queryClient.clear();
    }
  });
});
