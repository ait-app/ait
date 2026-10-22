// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PanGesture } from "react-native-gesture-handler/src/handlers/gestures/panGesture";
import { useMountReactions } from "react-native-gesture-handler/src/handlers/gestures/GestureDetector/useMountReactions";
import type { AttachedGestureState } from "react-native-gesture-handler/src/handlers/gestures/GestureDetector/types";
import { MountRegistry } from "react-native-gesture-handler/src/mountRegistry";

vi.mock("react-native", async (importOriginal) => {
  const original = await importOriginal<typeof import("react-native")>();
  return { ...original, Platform: { ...original.Platform, OS: "ios" } };
});

afterEach(cleanup);

const relations = ["wait", "simultaneous", "block"] as const;

function createRelatedDetectors(relation: (typeof relations)[number] = "wait") {
  const sharedRef: { current: PanGesture | undefined } = { current: undefined };
  const openingGesture = new PanGesture().withRef(sharedRef);
  openingGesture.initialize();
  const dependentGesture = new PanGesture();
  if (relation === "wait") {
    dependentGesture.requireExternalGestureToFail(sharedRef);
  } else if (relation === "simultaneous") {
    dependentGesture.simultaneousWithExternalGesture(sharedRef);
  } else {
    dependentGesture.blocksExternalGesture(sharedRef);
  }
  const state: AttachedGestureState = {
    attachedGestures: [dependentGesture],
    animatedEventHandler: null,
    animatedHandlers: null,
    shouldUseReanimated: false,
    isMounted: true,
  };
  return { sharedRef, openingGesture, state };
}

describe("native gesture detector replacement", () => {
  it.each(relations)(
    "ignores %s relations after layout cleanup and before passive cleanup",
    (relation) => {
      const { sharedRef, state } = createRelatedDetectors(relation);
      const updateDetector = vi.fn();
      renderHook(() => useMountReactions(updateDetector, state));

      // Folding replaces the shell's gesture hosts. Layout cleanup has detached the
      // old view, but its passive mount listener still sees the replacement's ref.
      state.isMounted = false;
      const replacement = new PanGesture().withRef(sharedRef);
      replacement.initialize();
      act(() => MountRegistry.gestureWillMount(replacement));

      expect(updateDetector).not.toHaveBeenCalled();
    },
  );

  it.each(relations)("still updates a live detector with a %s relation", (relation) => {
    const { sharedRef, state } = createRelatedDetectors(relation);
    const updateDetector = vi.fn();
    renderHook(() => useMountReactions(updateDetector, state));

    const replacement = new PanGesture().withRef(sharedRef);
    replacement.initialize();
    act(() => MountRegistry.gestureWillMount(replacement));

    expect(updateDetector).toHaveBeenCalledOnce();
  });

  it("removes the mount listener after passive cleanup", () => {
    const { openingGesture, state } = createRelatedDetectors();
    const updateDetector = vi.fn();
    const { unmount } = renderHook(() => useMountReactions(updateDetector, state));
    unmount();

    act(() => MountRegistry.gestureWillMount(openingGesture));

    expect(updateDetector).not.toHaveBeenCalled();
  });
});
