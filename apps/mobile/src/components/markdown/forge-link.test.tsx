// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { View } from "react-native";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ForgeSearchItem } from "@ait/protocol/messages";
import { AssistantFileLinkResolverProvider } from "@/assistant-file-links";
import { checkoutStatusQueryKey } from "@/git/query-keys";
import { forgeSearchQueryKey } from "@/git/use-forge-search-query";
import { WEB_RENDERERS } from "./renderers.web";

vi.mock("@/constants/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/constants/platform")>()),
  isWeb: true,
}));
vi.mock("@/constants/layout", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/constants/layout")>()),
  useIsCompactFormFactor: () => false,
}));
vi.mock("@gorhom/portal", () => ({
  Portal: ({ children }: React.PropsWithChildren) => children,
}));
vi.mock("@gorhom/bottom-sheet", () => ({ useBottomSheetModalInternal: () => null }));
vi.mock("@/hooks/use-hover-safe-zone", () => ({ useHoverSafeZone: () => undefined }));
vi.mock("@/hooks/use-is-local-daemon", () => ({ useIsLocalDaemon: () => false }));
// No host is connected, so the card can only show what a search already left in the cache.
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => null,
  useHostRuntimeIsConnected: () => false,
}));
vi.mock("@/components/ui/floating", async () => {
  const { forwardRef } = await import("react");
  const { View: RNView } = await import("react-native");
  return {
    FloatingSurface: forwardRef<View, React.ComponentProps<typeof RNView>>(
      function FloatingSurface(props, ref) {
        return <RNView ref={ref} testID={props.testID} children={props.children} />;
      },
    ),
  };
});

const SERVER_ID = "server";
const CWD = "/repo";
const OPEN_DELAY_MS = 350;
const PULL_REQUEST: ForgeSearchItem = {
  kind: "change_request",
  forge: "github",
  number: 795,
  title: "Show questionnaires one step at a time",
  url: "https://github.com/acme/app/pull/795",
  state: "merged",
  body: null,
  labels: [],
  projectPath: "acme/app",
  author: "octocat",
  additions: 963,
  deletions: 354,
  changedFiles: 15,
};

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function renderLink(url: string): HTMLElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(checkoutStatusQueryKey(SERVER_ID, CWD), {
    isGit: true,
    remoteUrl: "git@github.com:acme/app.git",
  });
  client.setQueryData(
    forgeSearchQueryKey(SERVER_ID, CWD, "795", ["change_request"], "github", "all"),
    { items: [PULL_REQUEST], authState: "authenticated", error: null, requestId: "search" },
  );
  const Link = WEB_RENDERERS.link!;
  render(
    <QueryClientProvider client={client}>
      <AssistantFileLinkResolverProvider serverId={SERVER_ID} workspaceRoot={CWD}>
        <p>
          <Link link={{ url }} anchorProps={{ href: url }}>
            the link
          </Link>
        </p>
      </AssistantFileLinkResolverProvider>
    </QueryClientProvider>,
  );
  return screen.getByText("the link");
}

function movePointer(link: HTMLElement, type: "pointerover" | "pointerout") {
  act(() => {
    fireEvent(link, new Event(type, { bubbles: true }));
  });
}

function rest(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

function card(): HTMLElement | null {
  return screen.queryByTestId("forge-link-card");
}

describe("a link to an issue or change request", () => {
  it("shows what it points at once the pointer rests on it, and puts it away after", () => {
    vi.useFakeTimers();
    const link = renderLink("https://github.com/acme/app/pull/795/files");

    movePointer(link, "pointerover");
    rest(OPEN_DELAY_MS - 1);
    expect(card()).toBeNull();
    rest(1);

    const text = card()!.textContent!;
    expect(text).toContain(PULL_REQUEST.title);
    expect(text).toContain("acme/app #795");
    expect(text).toContain("octocat");
    expect(text).toContain("+963");
    expect(text).toContain("-354");
    expect(screen.getByTestId("forge-link-card-state-merged")).toBeTruthy();

    movePointer(link, "pointerout");
    rest(200);
    expect(card()).toBeNull();
  });

  it("opens nothing for a pointer that only crosses it", () => {
    vi.useFakeTimers();
    const link = renderLink("https://github.com/acme/app/pull/795");

    movePointer(link, "pointerover");
    rest(OPEN_DELAY_MS - 50);
    movePointer(link, "pointerout");
    rest(1_000);
    expect(card()).toBeNull();
  });

  it.each(["https://github.com/other/repo/pull/795", "https://example.com/docs"])(
    "stays an ordinary link when it is %s, which the workspace cannot look up",
    (url) => {
      vi.useFakeTimers();
      const link = renderLink(url);

      movePointer(link, "pointerover");
      rest(1_000);
      expect(card()).toBeNull();
      expect(screen.queryByTestId("forge-link-hover-card")).toBeNull();
    },
  );
});
