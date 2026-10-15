// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePanelStore } from "@/stores/panel-store";
import { useOpenAgentListGesture, useOpenFileExplorerGesture } from "./gestures";

const system = vi.hoisted(() => ({
  listeners: new Set<() => boolean | null | undefined>(),
  emitEdge: null as ((event: { edge: "left" | "right" | null }) => void) | null,
  exitApp: vi.fn(),
  openExplorer: vi.fn(),
  blocked: { value: false },
  scrolledRight: { value: false },
  opener: { current: null as (() => void) | null },
}));

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {} },
}));

vi.mock("react-native", () => ({
  Platform: { OS: "android" },
  BackHandler: {
    addEventListener: (_event: string, listener: () => boolean | null | undefined) => {
      system.listeners.add(listener);
      return { remove: () => system.listeners.delete(listener) };
    },
  },
}));

vi.mock("react-native-gesture-handler", () => ({
  Gesture: {
    Pan: () => {
      const pan = {
        withRef: () => pan,
        simultaneousWithExternalGesture: () => pan,
        enabled: () => pan,
        manualActivation: () => pan,
        onTouchesDown: () => pan,
        onTouchesMove: () => pan,
        onStart: () => pan,
        onUpdate: () => pan,
        onEnd: () => pan,
      };
      return pan;
    },
  },
}));

vi.mock("react-native-reanimated", () => ({ useSharedValue: (value: unknown) => ({ value }) }));
vi.mock("react-native-worklets", () => ({ scheduleOnRN: vi.fn() }));
vi.mock("@/contexts/horizontal-scroll-context", () => ({
  useHorizontalScrollOptional: () => ({ isAnyScrolledRight: system.scrolledRight }),
}));
vi.mock("./provider", () => ({
  useMobilePanelsRuntime: () => ({
    fileExplorerOpener: system.opener,
    openGesturesBlocked: system.blocked,
  }),
}));
vi.mock("./system-back-source", () => ({
  subscribeSystemPanelBack: (listener: typeof system.emitEdge) => {
    system.emitEdge = listener;
    return () => {
      system.emitEdge = null;
    };
  },
}));

// Native edge metadata arrives before RN delivers hardwareBackPress. Unclaimed Back exits
// the root Activity; sheets and navigation use this same newest-listener-first chain.
function completeBack(edge: "left" | "right" | null) {
  system.emitEdge?.({ edge });
  for (const listener of [...system.listeners].reverse()) {
    if (listener()) return;
  }
  system.exitApp();
}

beforeEach(() => {
  vi.clearAllMocks();
  system.blocked.value = false;
  system.scrolledRight.value = false;
  system.opener.current = system.openExplorer;
  system.openExplorer.mockImplementation(() => {
    usePanelStore.setState({ mobilePanel: { target: "file-explorer", revision: 1 } });
  });
  usePanelStore.setState({ mobilePanel: { target: "agent", revision: 0 } });
});
afterEach(() => {
  cleanup();
  system.listeners.clear();
  system.emitEdge = null;
});

describe("Android edge swipes at the mobile gesture host", () => {
  it.each([
    ["left", "agent-list"],
    ["right", "file-explorer"],
  ] as const)("opens %s edge's %s panel instead of exiting", (edge, target) => {
    renderHook(() => useOpenAgentListGesture(true));

    act(() => completeBack(edge));

    expect(system.exitApp).not.toHaveBeenCalled();
    expect(usePanelStore.getState().mobilePanel.target).toBe(target);
  });

  it.each([
    ["agent-list", "right"],
    ["file-explorer", "left"],
  ] as const)("closes %s with an inward swipe from the %s edge", (target, edge) => {
    usePanelStore.setState({ mobilePanel: { target, revision: 1 } });
    renderHook(() => useOpenAgentListGesture(true));

    act(() => completeBack(edge));

    expect(system.exitApp).not.toHaveBeenCalled();
    expect(usePanelStore.getState().mobilePanel.target).toBe("agent");
  });

  it.each(["agent-list", "file-explorer"] as const)(
    "closes %s on the Back button before allowing navigation",
    (target) => {
      usePanelStore.setState({ mobilePanel: { target, revision: 1 } });
      renderHook(() => useOpenAgentListGesture(true));

      act(() => completeBack(null));

      expect(system.exitApp).not.toHaveBeenCalled();
      expect(usePanelStore.getState().mobilePanel.target).toBe("agent");
    },
  );

  it.each(["left", "right"] as const)(
    "respects the open-gesture blocker at the %s edge",
    (edge) => {
      system.blocked.value = true;
      renderHook(() => useOpenAgentListGesture(true));

      act(() => completeBack(edge));

      expect(system.exitApp).not.toHaveBeenCalled();
      expect(usePanelStore.getState().mobilePanel.target).toBe("agent");
    },
  );

  it("lets a newly opened sheet claim Back before changing panels", () => {
    renderHook(() => useOpenAgentListGesture(true));
    const dismissSheet = vi.fn(() => true);
    system.listeners.add(dismissSheet);

    act(() => completeBack("left"));

    expect(dismissSheet).toHaveBeenCalledOnce();
    expect(system.exitApp).not.toHaveBeenCalled();
    expect(usePanelStore.getState().mobilePanel.target).toBe("agent");
  });

  it("preserves ordinary Back navigation at the center surface", () => {
    renderHook(() => useOpenAgentListGesture(true));
    act(() => completeBack(null));
    expect(system.exitApp).toHaveBeenCalledOnce();
  });

  it.each([
    ["agent-list", "left"],
    ["file-explorer", "right"],
  ] as const)("keeps %s open when the edge swipe points away from the center", (target, edge) => {
    usePanelStore.setState({ mobilePanel: { target, revision: 1 } });
    renderHook(() => useOpenAgentListGesture(true));
    act(() => completeBack(edge));
    expect(system.exitApp).not.toHaveBeenCalled();
    expect(usePanelStore.getState().mobilePanel.target).toBe(target);
  });

  it("keeps horizontally scrolled content from opening the left panel", () => {
    system.scrolledRight.value = true;
    renderHook(() => useOpenAgentListGesture(true));
    act(() => completeBack("left"));
    expect(system.exitApp).not.toHaveBeenCalled();
    expect(usePanelStore.getState().mobilePanel.target).toBe("agent");
  });

  it("consumes the right edge even when no file explorer is available", () => {
    system.opener.current = null;
    renderHook(() => useOpenAgentListGesture(true));
    act(() => completeBack("right"));
    expect(system.exitApp).not.toHaveBeenCalled();
    expect(usePanelStore.getState().mobilePanel.target).toBe("agent");
  });

  it("does not reuse an edge claimed by a sheet on the next ordinary Back", () => {
    renderHook(() => useOpenAgentListGesture(true));
    const dismissSheet = () => true;
    system.listeners.add(dismissSheet);
    act(() => completeBack("right"));
    system.listeners.delete(dismissSheet);
    act(() => completeBack(null));
    expect(system.openExplorer).not.toHaveBeenCalled();
    expect(system.exitApp).toHaveBeenCalledOnce();
  });

  it("removes native metadata and Back subscriptions when the host unmounts", () => {
    const { unmount } = renderHook(() => useOpenAgentListGesture(true));
    expect(system.listeners.size).toBe(1);
    unmount();
    expect(system.listeners.size).toBe(0);
    expect(system.emitEdge).toBeNull();
  });

  it("registers only the enabled explorer's current open command", () => {
    system.opener.current = null;
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook((props) => useOpenFileExplorerGesture(props), {
      initialProps: { enabled: true, onOpen: first },
    });
    expect(system.opener.current).toBe(first);
    rerender({ enabled: true, onOpen: second });
    expect(system.opener.current).toBe(second);
    rerender({ enabled: false, onOpen: second });
    expect(system.opener.current).toBeNull();
  });

  it("does not install a panel Back handler when chrome is disabled", () => {
    renderHook(() => useOpenAgentListGesture(false));
    expect(system.listeners.size).toBe(0);
  });
});
