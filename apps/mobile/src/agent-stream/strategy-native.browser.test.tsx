import { act, createRef, forwardRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { View, type FlatList, type FlatListProps } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resolveStreamKeyboardInset } from "@/keyboard/shift/internal/policy";
import type { StreamItem } from "@/types/stream";
import { StreamTopOverlayProvider } from "./top-overlay";
import { createNativeStreamStrategy } from "./strategy-native";
import type { StreamRenderInput, StreamViewportHandle } from "./strategy";

let keyboardShift = 0;

vi.mock("react-native", async (importOriginal) => {
  const native = await importOriginal<typeof import("react-native")>();
  return {
    ...native,
    // Render the native strategy with RN Web's inverted FlatList. Browsers do
    // not implement UIKit's trailing contentInset; padding gives the same
    // resting scroll range for these geometry assertions.
    FlatList: forwardRef<FlatList<StreamItem>, FlatListProps<StreamItem>>(
      function InsetFlatList(props, ref) {
        return (
          <native.FlatList
            {...props}
            ref={ref}
            contentContainerStyle={[
              props.contentContainerStyle,
              { paddingBottom: props.contentInset?.bottom ?? 0 },
            ]}
          />
        );
      },
    ),
  };
});

vi.mock("@/keyboard/shift", () => ({
  useKeyboardStreamInset: () =>
    resolveStreamKeyboardInset({ platform: "ios", settledShift: keyboardShift }),
}));
vi.mock("./scroll-keyboard-dismiss/use-scroll-keyboard-dismiss", () => ({
  useScrollKeyboardDismiss: () => ({
    onScroll: () => {},
    onScrollBeginDrag: () => {},
    onScrollEndDrag: () => {},
  }),
}));

let root: Root | null = null;
let container: HTMLDivElement | null = null;

beforeEach(() => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
    value: true,
    configurable: true,
  });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  container?.remove();
  container = null;
});

function element(testId: string): HTMLElement {
  const node = container?.querySelector<HTMLElement>(`[data-testid="${testId}"]`);
  if (!node) throw new Error(`Missing ${testId}`);
  return node;
}

async function settleLayout() {
  await act(async () => {
    for (let frame = 0; frame < 8; frame += 1) {
      await new Promise(requestAnimationFrame);
    }
  });
}

describe("native inverted history under a floating header", () => {
  it.each([
    { headerHeight: 0, shift: 0 },
    { headerHeight: 112, shift: 0 },
    { headerHeight: 164, shift: 0 },
    { headerHeight: 0, shift: 311 },
    { headerHeight: 112, shift: 311 },
    { headerHeight: 164, shift: 311 },
  ])(
    "keeps the first message visible with header=$headerHeight and keyboard=$shift",
    async ({ headerHeight, shift }) => {
      keyboardShift = shift;
      const strategy = createNativeStreamStrategy();
      const history: StreamItem[] = Array.from({ length: 2 }, (_, index) => ({
        kind: "user_message",
        id: `message-${index}`,
        text: `Message ${index}`,
        timestamp: new Date("2026-10-21T00:00:00Z"),
      }));
      const renderRow = (item: StreamItem) => (
        <View
          testID={item.id}
          style={{ height: item.id === "message-0" ? 80 : 720, flexShrink: 0 }}
        />
      );
      container = document.createElement("div");
      Object.assign(container.style, {
        position: "relative",
        display: "flex",
        width: "390px",
        height: "600px",
      });
      document.body.append(container);
      root = createRoot(container);
      const input: StreamRenderInput = {
        agentId: "agent",
        segments: {
          historyVirtualized: [],
          historyMounted: strategy.orderTail(history),
          liveHead: [],
        },
        boundary: {
          hasVirtualizedHistory: false,
          hasMountedHistory: true,
          hasLiveHead: false,
        },
        renderers: {
          renderHistoryVirtualizedRow: renderRow,
          renderHistoryMountedRow: renderRow,
          renderLiveHeadRow: renderRow,
          renderLiveAuxiliary: () => null,
        },
        viewportRef: createRef<StreamViewportHandle>(),
        routeBottomAnchorRequest: null,
        isAuthoritativeHistoryReady: true,
        listEmptyComponent: null,
        onNearBottomChange: () => {},
        onNearHistoryStart: () => false,
        isLoadingOlderHistory: false,
        hasOlderHistory: false,
        olderHistoryProgressKey: null,
        scrollEnabled: true,
        listStyle: { flex: 1 },
        baseListContentContainerStyle: { flexGrow: 1 },
        forwardListContentContainerStyle: null,
        contentMaxWidth: 390,
      };
      const render = async (inset: number) => {
        await act(async () => {
          root?.render(
            <>
              <StreamTopOverlayProvider value={inset}>
                <View style={{ flex: 1, transform: [{ translateY: -shift }] }}>
                  {strategy.render(input)}
                </View>
              </StreamTopOverlayProvider>
              <div
                data-testid="floating-header"
                style={{ position: "absolute", top: 0, width: "100%", height: inset }}
              />
            </>,
          );
        });
        await settleLayout();
      };
      // The layout initially reports zero, then publishes the measured header.
      await render(0);
      await render(headerHeight);

      const viewport = element("agent-chat-scroll");
      expect(viewport.scrollHeight).toBeGreaterThan(viewport.clientHeight);
      // Positive native offsets run toward the oldest row in an inverted FlatList.
      // Scroll to the clamped resting limit, without relying on elastic overscroll.
      const restingOffset = viewport.scrollHeight - viewport.clientHeight;
      await act(async () => {
        viewport.scrollTop = restingOffset;
        // Wait for RN Web's end-of-scroll event as well as native anchor frames.
        await new Promise((resolve) => setTimeout(resolve, 150));
      });
      await settleLayout();
      expect(viewport.scrollTop).toBe(restingOffset);
      const firstMessage = element("message-0").getBoundingClientRect();
      expect(firstMessage.top).toBeGreaterThanOrEqual(
        element("floating-header").getBoundingClientRect().bottom,
      );
      expect(firstMessage.bottom).toBeLessThanOrEqual(viewport.getBoundingClientRect().bottom);
    },
  );
});
