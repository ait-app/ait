import { useRef, useState } from "react";
import { useStableEvent } from "@/hooks/use-stable-event";
import {
  abandonHistoryStartPaginationRequest,
  createHistoryStartPaginationState,
  evaluateHistoryStartPagination,
  rearmHistoryStartPagination,
  settleHistoryStartPagination,
  type HistoryStartPaginationInput,
  type HistoryStartPaginationTransition,
} from "./history-start-pagination";
import {
  createHistoryStartSettleScheduler,
  type HistoryStartSettleScheduler,
} from "./history-start-settle-scheduler";

const HISTORY_START_SETTLE_FRAMES = 2;

/**
 * Drives the history-start pagination state machine for a stream viewport. The viewport
 * supplies its measurements through `getInput`, returning null while it cannot measure.
 */
export function useHistoryStartPagination(input: {
  getInput: () => HistoryStartPaginationInput | null;
  olderHistoryProgressKey: string | null;
  onNearHistoryStart: () => boolean | Promise<boolean>;
}) {
  const { getInput, olderHistoryProgressKey, onNearHistoryStart } = input;
  const [state, setState] = useState(createHistoryStartPaginationState);
  const stateRef = useRef(state);
  const settleSchedulerRef = useRef<HistoryStartSettleScheduler | null>(null);

  const applyTransition = useStableEvent((transition: HistoryStartPaginationTransition) => {
    const previousState = stateRef.current;
    stateRef.current = transition.state;
    if (transition.state !== previousState) {
      setState(transition.state);
    }
    if (!transition.shouldLoad || olderHistoryProgressKey === null) {
      return;
    }
    const requestedProgressKey = olderHistoryProgressKey;
    void (async () => {
      const started = await onNearHistoryStart();
      if (started === true) {
        return;
      }
      applyTransition({
        state: abandonHistoryStartPaginationRequest(stateRef.current, requestedProgressKey),
        shouldLoad: false,
      });
    })();
  });
  const evaluate = useStableEvent(() => {
    const paginationInput = getInput();
    if (!paginationInput) {
      return;
    }
    applyTransition(evaluateHistoryStartPagination(stateRef.current, paginationInput));
  });
  const rearm = useStableEvent(() => {
    const rearmed = rearmHistoryStartPagination(stateRef.current);
    if (rearmed === stateRef.current) {
      return;
    }
    stateRef.current = rearmed;
    setState(rearmed);
    evaluate();
  });
  const reset = useStableEvent(() => {
    const initialState = createHistoryStartPaginationState();
    stateRef.current = initialState;
    setState(initialState);
  });
  const isSettling = useStableEvent(() => stateRef.current.status === "settling");
  const scheduleSettle = useStableEvent(() => {
    let scheduler = settleSchedulerRef.current;
    if (!scheduler) {
      scheduler = createHistoryStartSettleScheduler({
        settleFrames: HISTORY_START_SETTLE_FRAMES,
        requestFrame: (callback) => requestAnimationFrame(callback),
        cancelFrame: (frame) => cancelAnimationFrame(frame),
        isSettling,
        isLoading: () => {
          const paginationInput = getInput();
          return !paginationInput || paginationInput.isLoadingOlderHistory;
        },
        onSettle: () => {
          const paginationInput = getInput();
          if (!paginationInput) {
            return;
          }
          applyTransition(settleHistoryStartPagination(stateRef.current, paginationInput));
        },
      });
      settleSchedulerRef.current = scheduler;
    }
    scheduler.schedule();
  });
  const cancelSettle = useStableEvent(() => {
    settleSchedulerRef.current?.cancel();
  });

  return { state, evaluate, rearm, reset, isSettling, scheduleSettle, cancelSettle };
}
