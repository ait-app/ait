import { shouldAnimateNativeScrollToBottom } from "./native-scroll-animation";
import { Fragment, type ReactElement, useCallback, useEffect, useMemo, useRef } from "react";
import {
  FlatList,
  Platform,
  View,
  type LayoutChangeEvent,
  type ListRenderItemInfo,
  type NativeScrollEvent,
  type NativeSyntheticEvent,
  type ViewStyle,
} from "react-native";
import { withUnistyles } from "react-native-unistyles";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import type { StreamItem } from "@/types/stream";
import type { Theme } from "@/styles/theme";
import { useStableEvent } from "@/hooks/use-stable-event";
import { useKeyboardStreamInset } from "@/keyboard/shift";
import { useRevisedHistoryRows } from "./history-row-revision";
import { useStreamTopOverlayInset } from "./top-overlay";
import { useBottomAnchorController } from "./bottom-anchor-controller";
import { useScrollKeyboardDismiss } from "./scroll-keyboard-dismiss/use-scroll-keyboard-dismiss";
import type { StreamRenderInput, StreamStrategy, StreamViewportHandle } from "./strategy";
import { createStreamStrategy, resolveBottomAnchorTransportBehavior } from "./strategy";
import {
  isHistoryStartLoadingOperation,
  type HistoryStartPaginationInput,
} from "./history-start-pagination";
import { useHistoryStartPagination } from "./use-history-start-pagination";

const DEFAULT_MAINTAIN_VISIBLE_CONTENT_POSITION = Object.freeze({
  minIndexForVisible: 0,
  autoscrollToTopThreshold: 0,
});

const ThemedLoadingSpinner = withUnistyles(LoadingSpinner);
const foregroundMutedColorMapping = (theme: Theme) => ({
  color: theme.colors.foregroundMuted,
});
const historyStartSlotStyle: ViewStyle = {
  alignItems: "center",
  justifyContent: "center",
  height: 32,
  flexShrink: 0,
};

function keyExtractor(item: { id: string }): string {
  return item.id;
}

function NativeStreamViewport(props: StreamRenderInput & { strategy: StreamStrategy }) {
  const {
    agentId,
    segments,
    historyRowRevision,
    liveHeadRowRevision,
    boundary,
    renderers,
    listEmptyComponent,
    viewportRef,
    routeBottomAnchorRequest,
    isAuthoritativeHistoryReady,
    onNearBottomChange,
    onNearHistoryStart,
    isLoadingOlderHistory,
    hasOlderHistory,
    olderHistoryProgressKey,
    listStyle,
    baseListContentContainerStyle,
    strategy,
  } = props;
  const { renderHistoryMountedRow, renderLiveHeadRow, renderLiveAuxiliary } = renderers;
  const flatListRef = useRef<FlatList<StreamItem>>(null);
  const streamViewportMetricsRef = useRef({
    containerKey: "native-virtualized",
    contentHeight: 0,
    viewportWidth: 0,
    viewportHeight: 0,
    offsetY: 0,
    viewportMeasuredForKey: null as string | null,
    contentMeasuredForKey: null as string | null,
  });
  const scrollOffsetYRef = useRef(0);
  const isUserScrollActiveRef = useRef(false);
  const scrollKeyboardDismiss = useScrollKeyboardDismiss();
  const streamKeyboardInset = useKeyboardStreamInset();
  const topOverlayInset = useStreamTopOverlayInset();
  const userScrollEndFrameIdRef = useRef<number | null>(null);
  const programmaticScrollEventBudgetRef = useRef(0);
  const isNativeViewportSettlingRef = useRef(false);
  const nativeViewportSettlingFrameIdRef = useRef<number | null>(null);
  const historyStartReadyRef = useRef(false);

  const historyItems = useMemo(() => {
    if (segments.historyVirtualized.length === 0) {
      return segments.historyMounted;
    }
    return [...segments.historyVirtualized, ...segments.historyMounted];
  }, [segments.historyMounted, segments.historyVirtualized]);
  const historyRows = useRevisedHistoryRows(historyItems, historyRowRevision);
  const getHistoryStartPaginationInput = useStableEvent((): HistoryStartPaginationInput => {
    const metrics = streamViewportMetricsRef.current;
    const hasMeasuredViewport =
      metrics.viewportMeasuredForKey === metrics.containerKey &&
      metrics.contentMeasuredForKey === metrics.containerKey;
    return {
      distanceFromHistoryStart: metrics.contentHeight - metrics.viewportHeight - metrics.offsetY,
      hasOlderHistory,
      isLoadingOlderHistory,
      isReady: historyStartReadyRef.current && hasMeasuredViewport,
      progressKey: olderHistoryProgressKey,
    };
  });
  const historyStartPagination = useHistoryStartPagination({
    getInput: getHistoryStartPaginationInput,
    olderHistoryProgressKey,
    onNearHistoryStart,
  });
  const evaluateHistoryStart = historyStartPagination.evaluate;
  const scheduleHistoryStartSettle = historyStartPagination.scheduleSettle;

  const clearNativeViewportSettling = useCallback(() => {
    if (nativeViewportSettlingFrameIdRef.current !== null) {
      cancelAnimationFrame(nativeViewportSettlingFrameIdRef.current);
      nativeViewportSettlingFrameIdRef.current = null;
    }
  }, []);

  const clearPendingUserScrollEnd = useCallback(() => {
    if (userScrollEndFrameIdRef.current !== null) {
      cancelAnimationFrame(userScrollEndFrameIdRef.current);
      userScrollEndFrameIdRef.current = null;
    }
  }, []);

  const markNativeViewportSettling = useCallback(() => {
    clearNativeViewportSettling();
    isNativeViewportSettlingRef.current = true;
    let remainingFrames = 4;
    const tick = () => {
      if (remainingFrames <= 0) {
        nativeViewportSettlingFrameIdRef.current = null;
        isNativeViewportSettlingRef.current = false;
        return;
      }
      remainingFrames -= 1;
      nativeViewportSettlingFrameIdRef.current = requestAnimationFrame(tick);
    };
    nativeViewportSettlingFrameIdRef.current = requestAnimationFrame(tick);
  }, [clearNativeViewportSettling]);

  const getBottomAnchorTransportBehavior = useCallback(
    () =>
      resolveBottomAnchorTransportBehavior({
        strategy,
        isViewportSettling: isNativeViewportSettlingRef.current,
      }),
    [strategy],
  );

  const scrollToBottom = useCallback(
    (animated: boolean) => {
      programmaticScrollEventBudgetRef.current = 3;
      flatListRef.current?.scrollToOffset({
        offset: 0,
        animated:
          animated &&
          shouldAnimateNativeScrollToBottom({
            offsetY: scrollOffsetYRef.current,
            viewportHeight: streamViewportMetricsRef.current.viewportHeight,
            windowSize: 10,
          }),
      });
      scrollOffsetYRef.current = 0;
      streamViewportMetricsRef.current = {
        ...streamViewportMetricsRef.current,
        offsetY: 0,
      };
      onNearBottomChange(true);
    },
    [onNearBottomChange],
  );

  const bottomAnchorController = useBottomAnchorController({
    agentId,
    routeRequest: routeBottomAnchorRequest,
    isAuthoritativeHistoryReady,
    getTransportBehavior: getBottomAnchorTransportBehavior,
    getMeasurementState: () => streamViewportMetricsRef.current,
    isNearBottom: () => {
      const metrics = streamViewportMetricsRef.current;
      return strategy.isNearBottom({
        offsetY: metrics.offsetY,
        threshold: 32,
        contentHeight: metrics.contentHeight,
        viewportHeight: metrics.viewportHeight,
      });
    },
    scrollToBottom,
  });
  // Android's maintainVisibleContentPosition ignores the list inversion transform and
  // fights the controller's offset-zero correction while the live header grows.
  const maintainVisibleContentPosition =
    Platform.OS === "android" && bottomAnchorController.mode === "sticky-bottom"
      ? undefined
      : DEFAULT_MAINTAIN_VISIBLE_CONTENT_POSITION;
  const listContentContainerStyle = useMemo(
    () => [
      baseListContentContainerStyle,
      { paddingBottom: streamKeyboardInset.contentContainerPaddingBottom },
    ],
    [baseListContentContainerStyle, streamKeyboardInset.contentContainerPaddingBottom],
  );
  const listInsetProps = useMemo(
    () =>
      streamKeyboardInset.contentInset
        ? {
            automaticallyAdjustContentInsets: false,
            automaticallyAdjustsScrollIndicatorInsets: false,
            contentInsetAdjustmentBehavior: "never" as const,
            contentInset: streamKeyboardInset.contentInset,
            scrollIndicatorInsets: streamKeyboardInset.contentInset,
          }
        : {},
    [streamKeyboardInset.contentInset],
  );

  useEffect(() => {
    streamViewportMetricsRef.current = {
      containerKey: "native-virtualized",
      contentHeight: 0,
      viewportWidth: 0,
      viewportHeight: 0,
      offsetY: 0,
      viewportMeasuredForKey: null,
      contentMeasuredForKey: null,
    };
    scrollOffsetYRef.current = 0;
    isUserScrollActiveRef.current = false;
    clearPendingUserScrollEnd();
    clearNativeViewportSettling();
    isNativeViewportSettlingRef.current = false;
    historyStartReadyRef.current = false;
    historyStartPagination.reset();
    const frame = requestAnimationFrame(() => {
      historyStartReadyRef.current = true;
      evaluateHistoryStart();
    });
    return () => {
      cancelAnimationFrame(frame);
      clearPendingUserScrollEnd();
      historyStartPagination.cancelSettle();
    };
  }, [
    agentId,
    clearNativeViewportSettling,
    clearPendingUserScrollEnd,
    evaluateHistoryStart,
    historyStartPagination.reset,
    historyStartPagination.cancelSettle,
  ]);

  useEffect(() => () => clearNativeViewportSettling(), [clearNativeViewportSettling]);

  useEffect(() => {
    bottomAnchorController.prepareForStickyContentChange();
  }, [bottomAnchorController, historyRows, segments.liveHead]);

  useEffect(() => {
    const handle: StreamViewportHandle = {
      scrollToBottom: (reason = "jump-to-bottom") => {
        bottomAnchorController.requestLocalAnchor({
          agentId,
          reason,
        });
      },
      prepareForViewportChange: () => {
        bottomAnchorController.prepareForStickyViewportChange();
        markNativeViewportSettling();
      },
    };
    viewportRef.current = handle;
    return () => {
      if (viewportRef.current === handle) {
        viewportRef.current = null;
      }
    };
  }, [agentId, bottomAnchorController, markNativeViewportSettling, viewportRef]);

  const isScrollEventNearBottom = useStableEvent(
    (event: NativeSyntheticEvent<NativeScrollEvent>) => {
      const { contentOffset, contentSize, layoutMeasurement } = event.nativeEvent;
      return strategy.isNearBottom({
        offsetY: contentOffset.y,
        threshold: 32,
        contentHeight: contentSize.height,
        viewportHeight: layoutMeasurement.height,
      });
    },
  );

  const handleScroll = useStableEvent((event: NativeSyntheticEvent<NativeScrollEvent>) => {
    const { contentOffset, contentSize, layoutMeasurement } = event.nativeEvent;
    const previousOffsetY = scrollOffsetYRef.current;
    scrollOffsetYRef.current = contentOffset.y;
    scrollKeyboardDismiss.onScroll(event);

    streamViewportMetricsRef.current = {
      contentHeight: Math.max(0, contentSize.height),
      viewportWidth: Math.max(0, layoutMeasurement.width),
      viewportHeight: Math.max(0, layoutMeasurement.height),
      containerKey: "native-virtualized",
      offsetY: contentOffset.y,
      viewportMeasuredForKey: "native-virtualized",
      contentMeasuredForKey: "native-virtualized",
    };

    const nearBottom = isScrollEventNearBottom(event);
    onNearBottomChange(nearBottom);

    evaluateHistoryStart();

    if (
      !isUserScrollActiveRef.current &&
      programmaticScrollEventBudgetRef.current > 0 &&
      contentOffset.y <= 8
    ) {
      programmaticScrollEventBudgetRef.current -= 1;
    } else {
      programmaticScrollEventBudgetRef.current = 0;
      bottomAnchorController.handleScrollNearBottomChange({
        nextIsNearBottom: nearBottom,
        scrollDelta: contentOffset.y - previousOffsetY,
      });
    }
  });

  const handleScrollBeginDrag = useStableEvent((event: NativeSyntheticEvent<NativeScrollEvent>) => {
    clearPendingUserScrollEnd();
    isUserScrollActiveRef.current = true;
    scrollKeyboardDismiss.onScrollBeginDrag(event);
    bottomAnchorController.beginUserScroll();
    historyStartPagination.rearm();
  });

  // Defer drag end so momentum can take ownership, but capture the terminal
  // gesture position now because layout may move the viewport in the meantime.
  const handleScrollEndDrag = useStableEvent((event: NativeSyntheticEvent<NativeScrollEvent>) => {
    const isNearBottom = isScrollEventNearBottom(event);
    scrollKeyboardDismiss.onScrollEndDrag(event);

    clearPendingUserScrollEnd();
    userScrollEndFrameIdRef.current = requestAnimationFrame(() => {
      userScrollEndFrameIdRef.current = null;
      isUserScrollActiveRef.current = false;
      bottomAnchorController.endUserScroll({ isNearBottom });
    });
  });

  const handleMomentumScrollBegin = useStableEvent(() => {
    clearPendingUserScrollEnd();
  });

  const handleMomentumScrollEnd = useStableEvent(
    (event: NativeSyntheticEvent<NativeScrollEvent>) => {
      // Android can emit momentum-end after a programmatic anchor correction.
      // Only momentum that still owns the user gesture may settle scroll intent.
      if (!isUserScrollActiveRef.current) {
        return;
      }
      const isNearBottom = isScrollEventNearBottom(event);
      clearPendingUserScrollEnd();
      isUserScrollActiveRef.current = false;
      bottomAnchorController.endUserScroll({ isNearBottom });
    },
  );

  const handleListLayout = useStableEvent((event: LayoutChangeEvent) => {
    const previousViewportWidth = streamViewportMetricsRef.current.viewportWidth;
    const previousViewportHeight = streamViewportMetricsRef.current.viewportHeight;
    const viewportWidth = Math.max(0, event.nativeEvent.layout.width);
    const viewportHeight = Math.max(0, event.nativeEvent.layout.height);
    const viewportChanged =
      (previousViewportWidth > 0 && previousViewportWidth !== viewportWidth) ||
      (previousViewportHeight > 0 && previousViewportHeight !== viewportHeight);
    streamViewportMetricsRef.current = {
      ...streamViewportMetricsRef.current,
      containerKey: "native-virtualized",
      viewportWidth,
      viewportHeight,
      viewportMeasuredForKey: "native-virtualized",
    };
    if (viewportChanged) {
      markNativeViewportSettling();
    }
    bottomAnchorController.handleViewportMetricsChange({
      previousViewportWidth,
      viewportWidth,
      previousViewportHeight,
      viewportHeight,
    });
    evaluateHistoryStart();
  });

  const handleContentSizeChange = useStableEvent((_width: number, height: number) => {
    const previousContentHeight = streamViewportMetricsRef.current.contentHeight;
    const nextContentHeight = Math.max(0, height);
    streamViewportMetricsRef.current = {
      ...streamViewportMetricsRef.current,
      containerKey: "native-virtualized",
      contentHeight: nextContentHeight,
      contentMeasuredForKey: "native-virtualized",
    };
    bottomAnchorController.handleContentSizeChange({
      previousContentHeight,
      contentHeight: nextContentHeight,
    });
    evaluateHistoryStart();
    if (historyStartPagination.isSettling()) {
      scheduleHistoryStartSettle();
    }
  });

  useEffect(() => {
    evaluateHistoryStart();
    if (historyStartPagination.isSettling()) {
      scheduleHistoryStartSettle();
    }
  }, [
    evaluateHistoryStart,
    hasOlderHistory,
    isLoadingOlderHistory,
    olderHistoryProgressKey,
    scheduleHistoryStartSettle,
  ]);

  const renderItem = useStableEvent(
    ({ item, index }: ListRenderItemInfo<StreamItem>): ReactElement | null => {
      const rendered = renderHistoryMountedRow(item, index, historyItems);
      return (rendered ?? null) as ReactElement | null;
    },
  );

  const liveHeaderContent = useMemo(() => {
    // Stable render events read the latest expansion state; this revision makes
    // the memo invoke them again when that state changes.
    void liveHeadRowRevision;
    const liveHeadRows = segments.liveHead.map((item, index) => (
      <Fragment key={item.id}>{renderLiveHeadRow(item, index, segments.liveHead)}</Fragment>
    ));
    const liveAuxiliary = renderLiveAuxiliary();
    if (
      liveHeadRows.length === 0 &&
      !liveAuxiliary &&
      !boundary.hasMountedHistory &&
      !boundary.hasVirtualizedHistory
    ) {
      return (listEmptyComponent ?? null) as ReactElement | null;
    }
    return (
      <Fragment>
        {liveHeadRows}
        {liveAuxiliary}
      </Fragment>
    );
  }, [
    boundary,
    listEmptyComponent,
    liveHeadRowRevision,
    renderLiveAuxiliary,
    renderLiveHeadRow,
    segments.liveHead,
  ]);

  const historyFooterContent = useMemo(() => {
    const isLoadingOperation = isHistoryStartLoadingOperation(historyStartPagination.state);
    return (
      // The footer is the visual top of the inverted list. Reserve the floating
      // header here so the oldest message and loading spinner can rest below it.
      <View style={{ paddingTop: topOverlayInset }}>
        <View style={historyStartSlotStyle} testID="older-history-slot">
          {isLoadingOperation ? (
            <View testID="load-older-history-spinner">
              <ThemedLoadingSpinner size="small" uniProps={foregroundMutedColorMapping} />
            </View>
          ) : null}
        </View>
      </View>
    );
  }, [historyStartPagination.state, topOverlayInset]);

  // RN's FlatList strictMode keeps its internal renderItem wrapper stable when
  // data or the live header changes, preserving the row identities above.
  return (
    <FlatList
      {...listInsetProps}
      ref={flatListRef}
      data={historyRows}
      renderItem={renderItem}
      keyExtractor={keyExtractor}
      strictMode
      testID="agent-chat-scroll"
      nativeID="agent-chat-scroll-native-virtualized"
      ListHeaderComponent={liveHeaderContent ?? undefined}
      ListFooterComponent={historyFooterContent ?? undefined}
      contentContainerStyle={listContentContainerStyle}
      style={listStyle}
      onLayout={handleListLayout}
      onScroll={handleScroll}
      onScrollBeginDrag={handleScrollBeginDrag}
      onScrollEndDrag={handleScrollEndDrag}
      onMomentumScrollBegin={handleMomentumScrollBegin}
      onMomentumScrollEnd={handleMomentumScrollEnd}
      scrollEventThrottle={16}
      onContentSizeChange={handleContentSizeChange}
      maintainVisibleContentPosition={maintainVisibleContentPosition}
      initialNumToRender={12}
      windowSize={10}
      removeClippedSubviews={false}
      showsVerticalScrollIndicator
      inverted
    />
  );
}

export function createNativeStreamStrategy(): StreamStrategy {
  const strategy = createStreamStrategy({
    render: (renderInput) => <NativeStreamViewport {...renderInput} strategy={strategy} />,
    orderTailReverse: true,
    orderHeadReverse: true,
    assistantTurnTraversalStep: 1,
    historyLiveBoundaryEdge: "first",
    liveHeadHistoryBoundaryEdge: "last",
    frameChildOrder: "footer-then-content",
    flatListInverted: true,
    bottomAnchorTransportBehavior: {
      verificationDelayFrames: 2,
      verificationRetryMode: "recheck",
    },
    isNearBottom: (input) => input.offsetY <= input.threshold,
  });
  return strategy;
}
