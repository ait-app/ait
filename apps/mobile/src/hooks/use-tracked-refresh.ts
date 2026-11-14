import { useCallback, useRef, useState } from "react";

interface TrackedRefresh {
  isRefreshing: boolean;
  onRefresh: () => void;
}

/**
 * Shows a refresh indicator for exactly as long as a user-started refresh is in flight. The
 * indicator follows the refresh's own promise, so it clears whether the refresh succeeds or
 * fails, and an older refresh settling does not hide the indicator of a newer one.
 */
export function useTrackedRefresh(refresh: () => Promise<unknown>): TrackedRefresh {
  const [isRefreshing, setIsRefreshing] = useState(false);
  const latestRefreshRef = useRef(0);

  const onRefresh = useCallback(() => {
    const refreshId = latestRefreshRef.current + 1;
    latestRefreshRef.current = refreshId;
    setIsRefreshing(true);
    void refresh()
      .catch(() => undefined)
      .finally(() => {
        if (latestRefreshRef.current === refreshId) {
          setIsRefreshing(false);
        }
      });
  }, [refresh]);

  return { isRefreshing, onRefresh };
}
