import { describe, expect, it } from "vitest";
import type { StreamItem } from "@/types/stream";
import { resolveBottomAnchorTransportBehavior } from "./strategy";
import { resolveStreamRenderStrategy } from "./strategy-resolver";

function createTimestamp(seed: number): Date {
  return new Date(`2026-01-01T00:00:0${seed}.000Z`);
}

function userMessage(id: string, text: string, seed: number): StreamItem {
  return {
    kind: "user_message",
    id,
    text,
    timestamp: createTimestamp(seed),
  };
}

function assistantMessage(id: string, text: string, seed: number): StreamItem {
  return {
    kind: "assistant_message",
    id,
    text,
    timestamp: createTimestamp(seed),
  };
}

function toolCall(id: string, seed: number): StreamItem {
  return {
    kind: "tool_call",
    id,
    timestamp: createTimestamp(seed),
    payload: {
      source: "orchestrator",
      data: {
        toolCallId: id,
        toolName: "Shell",
        arguments: "echo hi",
        result: null,
        status: "completed",
      },
    },
  };
}

describe("resolveStreamRenderStrategy", () => {
  it("uses forward_stream on web", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });

    expect(strategy.getFlatListInverted()).toBe(false);
    expect(strategy.getBottomAnchorTransportBehavior()).toEqual({
      verificationDelayFrames: 0,
      verificationRetryMode: "rescroll",
    });
  });

  it("uses inverted_stream on native", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "ios",
      isMobileBreakpoint: false,
    });

    expect(strategy.getFlatListInverted()).toBe(true);
    expect(strategy.getBottomAnchorTransportBehavior()).toEqual({
      verificationDelayFrames: 2,
      verificationRetryMode: "recheck",
    });
  });

  it("delays native verification while viewport settling is in flight", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "ios",
      isMobileBreakpoint: false,
    });

    expect(
      resolveBottomAnchorTransportBehavior({
        strategy,
        isViewportSettling: true,
      }),
    ).toEqual({
      verificationDelayFrames: 4,
      verificationRetryMode: "recheck",
    });
  });

  it("does not inflate forward-stream verification delays during web resize", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });

    expect(
      resolveBottomAnchorTransportBehavior({
        strategy,
        isViewportSettling: true,
      }),
    ).toEqual({
      verificationDelayFrames: 0,
      verificationRetryMode: "rescroll",
    });
  });
});

describe("stream ordering", () => {
  const streamItems: StreamItem[] = [
    userMessage("u1", "user-1", 1),
    assistantMessage("a1", "assistant-1", 2),
    assistantMessage("a2", "assistant-2", 3),
  ];

  it("keeps forward_stream order unchanged for tail and head", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });

    const tail = strategy.orderTail(streamItems);
    const head = strategy.orderHead(streamItems);

    expect(tail.map((item) => item.id)).toEqual(["u1", "a1", "a2"]);
    expect(head.map((item) => item.id)).toEqual(["u1", "a1", "a2"]);
  });

  it("reverses inverted_stream order for tail and head", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "android",
      isMobileBreakpoint: false,
    });

    const tail = strategy.orderTail(streamItems);
    const head = strategy.orderHead(streamItems);

    expect(tail.map((item) => item.id)).toEqual(["a2", "a1", "u1"]);
    expect(head.map((item) => item.id)).toEqual(["a2", "a1", "u1"]);
  });
});

describe("neighbor and traversal semantics", () => {
  it("maps above/below indices for forward and inverted streams", () => {
    const forward = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });
    const inverted = resolveStreamRenderStrategy({
      platform: "ios",
      isMobileBreakpoint: false,
    });

    expect(forward.getNeighborIndex(3, "above")).toBe(2);
    expect(forward.getNeighborIndex(3, "below")).toBe(4);
    expect(inverted.getNeighborIndex(3, "above")).toBe(4);
    expect(inverted.getNeighborIndex(3, "below")).toBe(2);
  });

  it("collects assistant response content with strategy traversal direction", () => {
    const chronological: StreamItem[] = [
      userMessage("u1", "user-1", 1),
      assistantMessage("a1", "assistant-1", 2),
      assistantMessage("a2", "assistant-2", 3),
      userMessage("u2", "user-2", 4),
    ];

    const forward = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });
    const forwardStartIndex = chronological.findIndex((item) => item.id === "a2");
    expect(forward.collectAssistantResponseContent(chronological, forwardStartIndex)).toBe(
      "assistant-1\n\nassistant-2",
    );

    const inverted = resolveStreamRenderStrategy({
      platform: "android",
      isMobileBreakpoint: false,
    });
    const invertedItems = inverted.orderTail(chronological);
    const invertedStartIndex = invertedItems.findIndex((item) => item.id === "a2");
    expect(inverted.collectAssistantResponseContent(invertedItems, invertedStartIndex)).toBe(
      "assistant-1\n\nassistant-2",
    );
  });

  it("collects copy content across adjacent turns without a visible prompt", () => {
    const chronological: StreamItem[] = [
      { ...assistantMessage("a1", "first turn", 1), turnId: "turn-1" },
      { ...toolCall("tool", 2), turnId: "turn-2" },
      { ...assistantMessage("a2", "second turn", 3), turnId: "turn-2" },
    ];
    const strategy = resolveStreamRenderStrategy({ platform: "web", isMobileBreakpoint: false });

    expect(strategy.collectAssistantResponseContent(chronological, 2)).toBe(
      "first turn\n\nsecond turn",
    );
  });

  it("returns undefined neighbor when index would be out of bounds", () => {
    const forward = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });
    const items: StreamItem[] = [userMessage("u1", "user-1", 1)];

    expect(forward.getNeighborItem(items, 0, "above")).toBeUndefined();
    expect(forward.getNeighborItem(items, 0, "below")).toBeUndefined();
  });
});

describe("scroll/bottom calculations", () => {
  it("computes near-bottom using forward_stream distance-from-bottom math", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });

    expect(
      strategy.isNearBottom({
        offsetY: 680,
        viewportHeight: 300,
        contentHeight: 1000,
        threshold: 24,
      }),
    ).toBe(true);
    expect(
      strategy.isNearBottom({
        offsetY: 600,
        viewportHeight: 300,
        contentHeight: 1000,
        threshold: 24,
      }),
    ).toBe(false);
  });

  it("computes near-bottom for inverted_stream", () => {
    const strategy = resolveStreamRenderStrategy({
      platform: "ios",
      isMobileBreakpoint: false,
    });

    expect(
      strategy.isNearBottom({
        offsetY: 12,
        viewportHeight: 300,
        contentHeight: 1000,
        threshold: 24,
      }),
    ).toBe(true);
    expect(
      strategy.isNearBottom({
        offsetY: 40,
        viewportHeight: 300,
        contentHeight: 1000,
        threshold: 24,
      }),
    ).toBe(false);
  });
});

describe("layout strategy edges", () => {
  const streamItems: StreamItem[] = [
    userMessage("u1", "user-1", 1),
    assistantMessage("a1", "assistant-1", 2),
  ];

  it("uses the newest history edge as the history/live boundary", () => {
    const forward = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });
    const inverted = resolveStreamRenderStrategy({
      platform: "android",
      isMobileBreakpoint: false,
    });

    const forwardHistory = forward.orderTail(streamItems);
    const invertedHistory = inverted.orderTail(streamItems);

    expect(forward.getHistoryLiveBoundaryIndex(forwardHistory)).toBe(1);
    expect(inverted.getHistoryLiveBoundaryIndex(invertedHistory)).toBe(0);
  });

  it("uses the oldest live-head edge as the live-head/history boundary", () => {
    const forward = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });
    const inverted = resolveStreamRenderStrategy({
      platform: "ios",
      isMobileBreakpoint: false,
    });

    const forwardHead = forward.orderHead(streamItems);
    const invertedHead = inverted.orderHead(streamItems);

    expect(forward.getLiveHeadHistoryBoundaryIndex(forwardHead)).toBe(0);
    expect(inverted.getLiveHeadHistoryBoundaryIndex(invertedHead)).toBe(1);
  });

  it("names the frame child order needed by native inverted cells", () => {
    const forward = resolveStreamRenderStrategy({
      platform: "web",
      isMobileBreakpoint: false,
    });
    const inverted = resolveStreamRenderStrategy({
      platform: "android",
      isMobileBreakpoint: false,
    });

    expect(forward.getFrameChildOrder()).toBe("content-then-footer");
    expect(inverted.getFrameChildOrder()).toBe("footer-then-content");
  });
});
