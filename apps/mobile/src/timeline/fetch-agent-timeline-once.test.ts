import { expect, test } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { fetchAgentTimelineOnce } from "./fetch-agent-timeline-once";

type TimelinePage = Awaited<ReturnType<DaemonClient["fetchAgentTimeline"]>>;

test("concurrent identical timeline reads share one request", async () => {
  let resolvePage: (page: TimelinePage) => void = () => {};
  const page = new Promise<TimelinePage>((resolve) => {
    resolvePage = resolve;
  });
  const requests: { agentId: string; direction: string }[] = [];
  const client = {
    fetchAgentTimeline: async (
      agentId: string,
      request: Parameters<DaemonClient["fetchAgentTimeline"]>[1],
    ) => {
      requests.push({ agentId, direction: request?.direction ?? "tail" });
      return page;
    },
  };
  const request = { direction: "tail" as const, limit: 100, projection: "projected" as const };

  const first = fetchAgentTimelineOnce(client, "agent", request);
  const second = fetchAgentTimelineOnce(client, "agent", request);
  resolvePage({ hasNewer: false } as TimelinePage);

  await expect(Promise.all([first, second])).resolves.toHaveLength(2);
  expect(requests).toEqual([{ agentId: "agent", direction: "tail" }]);
});

test("startup timeline reads wait instead of overflowing a busy provider queue", async () => {
  let release!: () => void;
  const busy = new Promise<void>((resolve) => {
    release = resolve;
  });
  let active = 0;
  let rejected = 0;
  const client = {
    fetchAgentTimeline: async () => {
      if (active >= 4) {
        rejected += 1;
        throw new Error("resource_exhausted");
      }
      active += 1;
      await busy;
      active -= 1;
      return { hasNewer: false } as TimelinePage;
    },
  };
  const reads = Array.from({ length: 9 }, (_, index) =>
    fetchAgentTimelineOnce(client, `agent-${index}`, { direction: "tail" }),
  );
  const outcome = Promise.allSettled(reads);
  release();

  expect((await outcome).every((result) => result.status === "fulfilled")).toBe(true);
  expect(rejected).toBe(0);
});

test("queued duplicates share a read and failures release capacity for later agents", async () => {
  let release!: () => void;
  const busy = new Promise<void>((resolve) => {
    release = resolve;
  });
  const requested: string[] = [];
  const client = {
    fetchAgentTimeline: async (agentId: string) => {
      requested.push(agentId);
      await busy;
      if (agentId === "agent-0") throw new Error("history unavailable");
      return { hasNewer: false } as TimelinePage;
    },
  };
  const reads = Array.from({ length: 9 }, (_, index) =>
    fetchAgentTimelineOnce(client, `agent-${index}`, { direction: "tail" }),
  );
  const duplicate = fetchAgentTimelineOnce(client, "agent-8", { direction: "tail" });
  expect(duplicate).toBe(reads[8]);
  const outcome = Promise.allSettled(reads);
  release();

  const results = await outcome;
  expect(results[0]?.status).toBe("rejected");
  expect(results.slice(1).every((result) => result.status === "fulfilled")).toBe(true);
  expect(requested).toEqual(Array.from({ length: 9 }, (_, index) => `agent-${index}`));
});

test("a busy host does not delay timeline reads on another host", async () => {
  let release!: () => void;
  const busy = new Promise<void>((resolve) => {
    release = resolve;
  });
  const firstClient = {
    fetchAgentTimeline: async () => {
      await busy;
      return { hasNewer: false } as TimelinePage;
    },
  };
  const secondClient = {
    fetchAgentTimeline: async () => ({ hasNewer: false }) as TimelinePage,
  };
  const first = Array.from({ length: 9 }, (_, index) =>
    fetchAgentTimelineOnce(firstClient, `agent-${index}`, { direction: "tail" }),
  );

  await expect(
    fetchAgentTimelineOnce(secondClient, "agent", { direction: "tail" }),
  ).resolves.toEqual({
    hasNewer: false,
  });
  release();
  await Promise.all(first);
});
