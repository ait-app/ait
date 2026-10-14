import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { TIMELINE_FETCH_PAGE_SIZE } from "./timeline-fetch-policy";

type TimelineRequest = Parameters<DaemonClient["fetchAgentTimeline"]>[1];
type TimelinePage = Awaited<ReturnType<DaemonClient["fetchAgentTimeline"]>>;

interface TimelinePagePorts {
  scope: object;
  supportsCachedReads: boolean;
  fetch: (request: TimelineRequest) => Promise<TimelinePage>;
  apply: (page: TimelinePage) => void;
  isCurrent: () => boolean;
  reportError: (error: unknown) => void;
}

const revalidations = new WeakMap<object, Map<string, Promise<TimelinePage>>>();

function revalidate(agentId: string, ports: TimelinePagePorts): void {
  let pending = revalidations.get(ports.scope);
  if (!pending) {
    pending = new Map();
    revalidations.set(ports.scope, pending);
  }
  let refresh = pending.get(agentId);
  if (!refresh) {
    refresh = Promise.resolve().then(() =>
      ports.fetch({
        direction: "tail",
        limit: TIMELINE_FETCH_PAGE_SIZE,
        projection: "projected",
        allowCached: false,
      }),
    );
    pending.set(agentId, refresh);
    const clear = () => {
      if (pending.get(agentId) === refresh) pending.delete(agentId);
    };
    void refresh.then(clear, clear);
  }
  void refresh
    .then((page) => {
      if (ports.isCurrent()) ports.apply(page);
    })
    .catch((error) => {
      if (ports.isCurrent()) ports.reportError(error);
    });
}

/** Show a durable page immediately, then reconcile native history without delaying its caller. */
export async function fetchAgentTimelinePage(
  agentId: string,
  request: TimelineRequest,
  ports: TimelinePagePorts,
): Promise<TimelinePage> {
  const allowCached = ports.supportsCachedReads && request?.allowCached !== false;
  const { allowCached: _allowCached, ...options } = request ?? {};
  const page = await ports.fetch({ ...options, ...(allowCached ? { allowCached: true } : {}) });
  if (ports.isCurrent()) {
    ports.apply(page);
    if (page.historyNeedsRefresh && allowCached) revalidate(agentId, ports);
  }
  return page;
}
