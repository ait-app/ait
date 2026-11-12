// @vitest-environment jsdom
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppNavigationGestureSurface } from "@/components/app-navigation-gesture-surface";

const { useOpenGesture } = vi.hoisted(() => ({ useOpenGesture: vi.fn(() => ({})) }));
vi.mock("@/constants/platform", () => ({ isWeb: false }));
vi.mock("@/mobile-panels/gestures", () => ({ useOpenAgentListGesture: useOpenGesture }));
vi.mock("react-native-gesture-handler", () => ({
  GestureDetector: ({ children }: { children: React.ReactNode }) => children,
}));
afterEach(cleanup);

function TaskRoute() {
  const [detailOpen, setDetailOpen] = useState(false);
  return (
    <button onClick={() => setDetailOpen(true)}>
      {detailOpen ? "Completed task detail" : "Completed tasks"}
    </button>
  );
}

describe("native navigation gesture surface", () => {
  it("retains the selected route and detail state across repeated folds", () => {
    const view = (isCompact: boolean) => (
      <AppNavigationGestureSurface isCompact={isCompact} chromeEnabled>
        <TaskRoute />
      </AppNavigationGestureSurface>
    );
    const { rerender } = render(view(false));
    fireEvent.click(screen.getByText("Completed tasks"));
    for (const compact of [true, false, true]) {
      act(() => rerender(view(compact)));
      expect(screen.getByText("Completed task detail")).toBeTruthy();
      expect(useOpenGesture).toHaveBeenLastCalledWith(compact);
    }
  });
});
