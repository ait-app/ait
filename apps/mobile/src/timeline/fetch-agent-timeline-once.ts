import type { DaemonClient } from "@ait/client/internal/daemon-client";

type TimelineClient = Pick<DaemonClient, "fetchAgentTimeline">;
type TimelineRequest = Parameters<TimelineClient["fetchAgentTimeline"]>[1];
type TimelinePage = Awaited<ReturnType<TimelineClient["fetchAgentTimeline"]>>;

// Leave room for subscriptions and controls in the daemon's bounded provider queue.
const MAX_CONCURRENT_TIMELINE_READS = 4;

interface TimelineReads {
  inFlight: Map<string, Promise<TimelinePage>>;
  active: number;
  waiting: Set<() => void>;
}

const readsByClient = new WeakMap<object, TimelineReads>();

export function fetchAgentTimelineOnce(
  client: TimelineClient,
  agentId: string,
  request: TimelineRequest,
): Promise<TimelinePage> {
  let reads = readsByClient.get(client);
  if (!reads) {
    reads = { inFlight: new Map(), active: 0, waiting: new Set() };
    readsByClient.set(client, reads);
  }
  const { inFlight } = reads;

  const key = `${agentId}:${JSON.stringify(request)}`;
  const existing = inFlight.get(key);
  if (existing) return existing;

  const queue = reads;
  const fetch = new Promise<TimelinePage>((resolve, reject) => {
    const start = () => {
      queue.active += 1;
      void Promise.resolve()
        .then(() => client.fetchAgentTimeline(agentId, request))
        .then(resolve, reject)
        .finally(() => {
          queue.active -= 1;
          const next = queue.waiting.values().next().value;
          if (next) {
            queue.waiting.delete(next);
            next();
          }
        });
    };
    if (queue.active < MAX_CONCURRENT_TIMELINE_READS) start();
    else queue.waiting.add(start);
  });
  inFlight.set(key, fetch);
  const clear = () => {
    if (inFlight.get(key) === fetch) inFlight.delete(key);
  };
  void fetch.then(clear, clear);
  return fetch;
}
