/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { getSidebarRowBackdrop } from "./sidebar-row-backdrop";
import { useSidebarRowPress } from "./use-sidebar-row-press";

vi.mock("@/constants/platform", () => ({ isNative: true }));
afterEach(cleanup);

it("matches the badge backdrop to native highlight and resets it when the gesture is cancelled", () => {
  const { result } = renderHook(useSidebarRowPress);
  const backdrop = () => getSidebarRowBackdrop({ isPressed: result.current.isHighlighted });
  expect(backdrop()).toBe("surfaceSidebar");
  act(() => result.current.onHighlightChange?.(true));
  expect(backdrop()).toBe("surface2");
  expect(result.current.isPressed).toBe(false);
  act(() => result.current.onHighlightChange?.(false));
  expect(backdrop()).toBe("surfaceSidebar");
});
