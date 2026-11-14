import type { ReactNode, RefObject } from "react";
import type { StyleProp, ViewStyle } from "react-native";
import type { StreamItem } from "@/types/stream";
import { continuesResponse } from "./turn-membership";
import type { StreamHistoryBoundary, StreamRenderSegments } from "./model";
import type {
  BottomAnchorLocalRequest,
  BottomAnchorRouteRequest,
} from "./bottom-anchor-controller";

type NeighborRelation = "above" | "below";
type AssistantTurnTraversalStep = -1 | 1;
export type StreamFrameChildOrder = "content-then-footer" | "footer-then-content";

export type BottomAnchorTransportBehavior = Readonly<{
  verificationDelayFrames: number;
  verificationRetryMode: "rescroll" | "recheck";
}>;

export interface StreamNearBottomInput {
  contentHeight: number;
  viewportHeight: number;
  offsetY: number;
  threshold: number;
}

/**
 * A caller that landed on one occurrence inside the message owns where the viewport
 * settles. It resolved the occurrence against a specific row already, so it takes no
 * row here and returns null once that row is gone.
 */
export interface ScrollToMessageOccurrence {
  signal: AbortSignal;
  targetTop(): number | null;
}

export interface StreamViewportHandle {
  /** Give keyboard navigation back to the scrolling surface on web. */
  focus?: () => void;
  scrollToBottom: (reason?: BottomAnchorLocalRequest["reason"]) => void;
  prepareForViewportChange: () => void;
  scrollToMessage?: (messageId: string, occurrence?: ScrollToMessageOccurrence) => void;
}

export interface StreamSegmentRenderers {
  renderHistoryVirtualizedRow: (item: StreamItem, index: number, items: StreamItem[]) => ReactNode;
  renderHistoryMountedRow: (item: StreamItem, index: number, items: StreamItem[]) => ReactNode;
  renderLiveHeadRow: (item: StreamItem, index: number, items: StreamItem[]) => ReactNode;
  renderLiveAuxiliary: () => ReactNode;
}

export interface StreamHistoryRowRevision {
  contentById: { has(id: string): boolean };
  displayStateById: { has(id: string): boolean };
  globalDisplayState: boolean;
}

export interface StreamRenderInput {
  imageContext?: import("@/utils/assistant-image-metadata").AssistantImageContext;
  agentId: string;
  segments: StreamRenderSegments;
  historyRowRevision?: StreamHistoryRowRevision;
  liveHeadRowRevision?: unknown;
  boundary: StreamHistoryBoundary;
  renderers: StreamSegmentRenderers;
  listEmptyComponent: ReactNode;
  viewportRef: RefObject<StreamViewportHandle | null>;
  routeBottomAnchorRequest: BottomAnchorRouteRequest | null;
  isAuthoritativeHistoryReady: boolean;
  onNearBottomChange: (value: boolean) => void;
  // The history row under the top of the viewport, for surfaces that mark where the reader
  // is in the transcript. Only the web viewport measures it today.
  onReadingPositionChange?: (rowId: string | null) => void;
  onNearHistoryStart: () => boolean | Promise<boolean>;
  isLoadingOlderHistory: boolean;
  hasOlderHistory: boolean;
  olderHistoryProgressKey: string | null;
  listStyle: StyleProp<ViewStyle>;
  baseListContentContainerStyle: StyleProp<ViewStyle>;
  // The chat column's max width. The web viewport estimates unmeasured row heights with it.
  contentMaxWidth: number;
}

export interface ResolveStreamRenderStrategyInput {
  platform: string;
  isMobileBreakpoint: boolean;
}

export interface StreamStrategy {
  render: (input: StreamRenderInput) => ReactNode;
  orderTail: (streamItems: StreamItem[]) => StreamItem[];
  orderHead: (streamHead: StreamItem[]) => StreamItem[];
  getNeighborIndex: (index: number, relation: NeighborRelation) => number;
  getNeighborItem: (
    items: StreamItem[],
    index: number,
    relation: NeighborRelation,
  ) => StreamItem | undefined;
  collectAssistantResponseContent: (items: StreamItem[], startIndex: number) => string;
  isNearBottom: (input: StreamNearBottomInput) => boolean;
  getBottomAnchorTransportBehavior: () => BottomAnchorTransportBehavior;
  getHistoryLiveBoundaryIndex: (history: StreamItem[]) => number | null;
  getLiveHeadHistoryBoundaryIndex: (liveHead: StreamItem[]) => number | null;
  getLatestItemIndex: (items: StreamItem[]) => number | null;
  getFrameChildOrder: () => StreamFrameChildOrder;
  getFlatListInverted: () => boolean;
}

interface StreamStrategyConfig {
  render: StreamStrategy["render"];
  orderTailReverse: boolean;
  orderHeadReverse: boolean;
  assistantTurnTraversalStep: AssistantTurnTraversalStep;
  historyLiveBoundaryEdge: "first" | "last";
  liveHeadHistoryBoundaryEdge: "first" | "last";
  frameChildOrder: StreamFrameChildOrder;
  flatListInverted: boolean;
  bottomAnchorTransportBehavior: BottomAnchorTransportBehavior;
  isNearBottom: (input: StreamNearBottomInput) => boolean;
}

const NATIVE_SETTLING_VERIFICATION_DELAY_FRAMES = 4;

export function createStreamStrategy(config: StreamStrategyConfig): StreamStrategy {
  const getNeighborIndex = (index: number, relation: NeighborRelation) =>
    relation === "above"
      ? index + config.assistantTurnTraversalStep
      : index - config.assistantTurnTraversalStep;
  return {
    render: config.render,
    orderTail: (streamItems) =>
      config.orderTailReverse ? [...streamItems].toReversed() : streamItems,
    orderHead: (streamHead) =>
      config.orderHeadReverse ? [...streamHead].toReversed() : streamHead,
    getNeighborIndex,
    getNeighborItem: (items, index, relation) => {
      const neighborIndex = getNeighborIndex(index, relation);
      if (neighborIndex < 0 || neighborIndex >= items.length) {
        return undefined;
      }
      return items[neighborIndex];
    },
    collectAssistantResponseContent: (items, startIndex) => {
      const messages: string[] = [];
      let laterItem: StreamItem | null = null;
      for (
        let index = startIndex;
        index >= 0 && index < items.length;
        index += config.assistantTurnTraversalStep
      ) {
        const currentItem = items[index];
        if (!currentItem || (laterItem && !continuesResponse(currentItem, laterItem))) {
          break;
        }
        if (currentItem.kind === "assistant_message") {
          messages.push(currentItem.text);
        }
        laterItem = currentItem;
      }
      return messages.toReversed().join("\n\n");
    },
    isNearBottom: config.isNearBottom,
    getBottomAnchorTransportBehavior: () => config.bottomAnchorTransportBehavior,
    getHistoryLiveBoundaryIndex: (history) => {
      if (history.length === 0) {
        return null;
      }
      return config.historyLiveBoundaryEdge === "first" ? 0 : history.length - 1;
    },
    getLiveHeadHistoryBoundaryIndex: (liveHead) => {
      if (liveHead.length === 0) {
        return null;
      }
      return config.liveHeadHistoryBoundaryEdge === "first" ? 0 : liveHead.length - 1;
    },
    getLatestItemIndex: (items) => {
      if (items.length === 0) {
        return null;
      }
      return config.historyLiveBoundaryEdge === "first" ? 0 : items.length - 1;
    },
    getFrameChildOrder: () => config.frameChildOrder,
    getFlatListInverted: () => config.flatListInverted,
  };
}

export function resolveBottomAnchorTransportBehavior(input: {
  strategy: StreamStrategy;
  isViewportSettling: boolean;
}): BottomAnchorTransportBehavior {
  const baseBehavior = input.strategy.getBottomAnchorTransportBehavior();
  if (!input.isViewportSettling || !input.strategy.getFlatListInverted()) {
    return baseBehavior;
  }
  return {
    verificationDelayFrames: Math.max(
      baseBehavior.verificationDelayFrames,
      NATIVE_SETTLING_VERIFICATION_DELAY_FRAMES,
    ),
    verificationRetryMode: "recheck",
  };
}
