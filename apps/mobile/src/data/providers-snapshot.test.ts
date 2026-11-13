import { createRequire } from "node:module";
import { QueryClient, QueryObserver, CancelledError } from "@tanstack/react-query";
import type { AgentFeature, ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import type { GetProvidersSnapshotResponseMessage } from "@ait/protocol/messages";
import { compactProviderSnapshot } from "@ait/protocol/provider-snapshot-codec";
import { applyProvidersSnapshotUpdate } from "./push-router";
import {
  createProviderSnapshotCache,
  ProviderSnapshotCacheMissError,
} from "./provider-snapshot-cache";
import {
  resolveProviderIconName,
  replaceProviderSnapshotIcons,
} from "@/components/provider-icon-name";
import { describe, expect, it, vi } from "vitest";
import {
  fetchProvidersSnapshot,
  isProvidersSnapshotHomeScope,
  normalizeProvidersSnapshotCwd,
  providersSnapshotQueryKey,
  providersSnapshotQueryRoot,
  providersSnapshotRequestOptions,
  refreshAndApplyProvidersSnapshot,
  refreshProvidersOnAccess,
} from "@/data/providers-snapshot";

describe("providers snapshot query scope", () => {
  it("normalizes blank cwd values to the home scope", () => {
    expect(normalizeProvidersSnapshotCwd(undefined)).toBeNull();
    expect(normalizeProvidersSnapshotCwd(null)).toBeNull();
    expect(normalizeProvidersSnapshotCwd("   ")).toBeNull();
    expect(isProvidersSnapshotHomeScope("")).toBe(true);
  });

  it("keeps home and workspace query keys separate under one server root", () => {
    expect(providersSnapshotQueryRoot("server-1")).toEqual(["providersSnapshot", "server-1"]);
    expect(providersSnapshotQueryKey("server-1")).toEqual([
      "providersSnapshot",
      "server-1",
      "home",
    ]);
    expect(providersSnapshotQueryKey("server-1", "/repo-a")).toEqual([
      "providersSnapshot",
      "server-1",
      "cwd",
      "/repo-a",
    ]);
  });

  it("builds request options with cwd only for workspace scopes", () => {
    expect(providersSnapshotRequestOptions({ cwd: null, providers: ["codex"] })).toEqual({
      providers: ["codex"],
    });
    expect(providersSnapshotRequestOptions({ cwd: "/repo-a", providers: ["codex"] })).toEqual({
      cwd: "/repo-a",
      providers: ["codex"],
    });
  });

  it("uses one query scope for Windows cwd values with either separator", () => {
    expect(normalizeProvidersSnapshotCwd("C:\\Users\\Ezekiel Bulver\\project")).toBe(
      "C:/Users/Ezekiel Bulver/project",
    );
    expect(providersSnapshotQueryKey("server-1", "C:\\Users\\Ezekiel Bulver\\project")).toEqual(
      providersSnapshotQueryKey("server-1", "C:/Users/Ezekiel Bulver/project"),
    );
  });
});

describe("provider snapshot revisions and background refresh", () => {
  it("keeps a newer published revision when an older GET or push arrives", async () => {
    const queryClient = new QueryClient();
    const cache = createProviderSnapshotCache(createStorage());
    const serverId = "revision-fence";
    const key = providersSnapshotQueryKey(serverId, snapshot.cwd);
    const latest = { ...snapshot, entries, generation: "daemon-1", revision: 5 };
    queryClient.setQueryData(key, latest);
    try {
      const result = await fetchProvidersSnapshot({
        client: {
          getProvidersSnapshot: async () => ({
            ...snapshot,
            generation: "daemon-1",
            revision: 4,
          }),
        },
        serverId,
        cwd: snapshot.cwd,
        queryClient,
        cache,
      });
      expect(result).toBe(latest);
      await applyProvidersSnapshotUpdate({
        client: { getProvidersSnapshot: async () => snapshot },
        serverId,
        queryClient,
        cache,
        message: {
          type: "provider.snapshot.update",
          payload: { ...snapshot, generation: "daemon-1", revision: 3 },
        },
      });
      expect(queryClient.getQueryData(key)).toBe(latest);
      const restarted = await fetchProvidersSnapshot({
        client: {
          getProvidersSnapshot: async () => ({
            ...snapshot,
            generation: "daemon-2",
            revision: 1,
          }),
        },
        serverId,
        cwd: snapshot.cwd,
        queryClient,
        cache,
      });
      expect(restarted.generation).toBe("daemon-2");
      expect(restarted.revision).toBe(1);
    } finally {
      queryClient.clear();
    }
  });

  it.each(["ready", "error"] as const)(
    "waits beyond refresh acceptance for the %s terminal snapshot",
    async (status) => {
      vi.useFakeTimers();
      const queryClient = new QueryClient();
      const cache = createProviderSnapshotCache(createStorage());
      const serverId = `refresh-${status}`;
      let release = () => {};
      let fetched = () => {};
      const initialRead = new Promise<void>((resolve) => {
        fetched = resolve;
      });
      const terminal = new Promise<void>((resolve) => {
        release = resolve;
      });
      let calls = 0;
      let finished = false;
      const refresh = refreshAndApplyProvidersSnapshot({
        client: {
          refreshProvidersSnapshot: async () => ({
            requestId: "refresh",
            acknowledged: true,
            generation: "daemon",
            revision: 1,
          }),
          getProvidersSnapshot: async () => {
            calls += 1;
            if (calls === 1) {
              fetched();
              return {
                ...snapshot,
                generation: "daemon",
                revision: 1,
                refreshing: ["test-provider"],
              };
            }
            await terminal;
            return {
              ...snapshot,
              snapshotHash: `terminal-${status}`,
              compactSnapshot: compactProviderSnapshot([{ ...entries[0]!, status }]),
              generation: "daemon",
              revision: 2,
              refreshing: [],
            };
          },
        },
        serverId,
        cwd: snapshot.cwd,
        providers: ["test-provider"],
        queryClient,
        cache,
      }).then((result) => {
        finished = true;
        return result;
      });
      try {
        await initialRead;
        await vi.advanceTimersByTimeAsync(200);
        expect(finished).toBe(false);
        release();
        await refresh;
        expect(calls).toBe(2);
        expect(
          queryClient.getQueryData(providersSnapshotQueryKey(serverId, snapshot.cwd)),
        ).toMatchObject({ entries: [{ status }], revision: 2, refreshing: [] });
      } finally {
        release();
        queryClient.clear();
        vi.useRealTimers();
      }
    },
  );
});

// Resolve the exact CommonJS implementation installed by React Native's setUpXHR.
// Its runtime signal deliberately lacks the newer methods in the DOM typings.
const nativeRequire = createRequire(require.resolve("react-native/package.json"));
const { AbortController: NativeAbortController }: { AbortController: typeof AbortController } =
  nativeRequire("abort-controller/dist/abort-controller");

function createStorage(onWrite: () => void = () => {}) {
  const values = new Map<string, string>();
  return {
    async getItem(key: string) {
      return values.get(key) ?? null;
    },
    async setItem(key: string, value: string) {
      values.set(key, value);
      onWrite();
    },
    async removeItem(key: string) {
      values.delete(key);
    },
    async getAllKeys() {
      return [...values.keys()];
    },
    async multiGet(keys: readonly string[]) {
      return keys.map((key) => [key, values.get(key) ?? null] as const);
    },
    async multiRemove(keys: readonly string[]) {
      for (const key of keys) values.delete(key);
    },
  };
}

const entries: ProviderSnapshotEntry[] = [
  {
    provider: "test-provider",
    status: "ready",
    enabled: true,
    iconSvg: '<svg xmlns="http://www.w3.org/2000/svg"/>',
    models: [
      {
        provider: "test-provider",
        id: "test-model",
        label: "Test model",
        thinkingOptions: [],
      },
    ],
  },
];
const snapshot = {
  requestId: "test-request",
  entries: [],
  cwd: "/repo",
  generatedAt: "2026-09-07T00:00:00.000Z",
  snapshotHash: "test-hash",
  compactSnapshot: compactProviderSnapshot(entries),
} satisfies GetProvidersSnapshotResponseMessage["payload"];

describe("role discovery follows completed model catalogs", () => {
  const roles: AgentFeature[] = [
    { type: "select", id: "modelRole.task", label: "Task", value: "native", options: [] },
  ];

  it.each(["first-ready", "bootstrap", "refresh"] as const)(
    "repairs a cached empty role list after %s without touching other scopes",
    async (transition) => {
      const queryClient = new QueryClient();
      const cache = createProviderSnapshotCache(createStorage());
      const serverId = `role-catalog-${transition}`;
      const provider = entries[0]!.provider;
      const key = providersSnapshotQueryKey(serverId, snapshot.cwd);
      const entry = { ...entries[0]!, fetchedAt: "2026-09-07T00:00:01.000Z" };
      const ready = {
        ...snapshot,
        entries: [entry],
        compactSnapshot: compactProviderSnapshot([entry]),
        snapshotHash: "role-catalog-ready",
        generation: "daemon",
        revision: 2,
        refreshing: [],
      };
      if (transition !== "first-ready")
        queryClient.setQueryData(key, {
          ...ready,
          entries: [
            {
              ...entry,
              status: transition === "bootstrap" ? "loading" : "ready",
              fetchedAt: "2026-09-07T00:00:00.000Z",
            },
          ],
          revision: 1,
        });
      const roleKey = ["providerPresetFeatures", serverId, provider, snapshot.cwd];
      const untouched = [
        ["providerPresetFeatures", "another-host", provider, snapshot.cwd],
        ["providerPresetFeatures", serverId, "another-provider", snapshot.cwd],
        ["providerPresetFeatures", serverId, provider, "/another-repo"],
        ["provider-presets", serverId, provider, snapshot.cwd],
      ];
      for (const other of untouched) queryClient.setQueryData(other, []);
      const readRoles = vi.fn(async () => roles);
      const observer = new QueryObserver(queryClient, {
        queryKey: roleKey,
        queryFn: readRoles,
        initialData: [] as AgentFeature[],
        staleTime: 60_000,
      });
      let markRepaired = () => {};
      const repaired = new Promise<void>((resolve) => {
        markRepaired = resolve;
      });
      const unsubscribe = observer.subscribe((result) => {
        if (result.data?.length && !result.isFetching) markRepaired();
      });
      const client = {
        getProvidersSnapshot: async () => ready,
        refreshProvidersSnapshot: async () => ({
          requestId: "refresh",
          acknowledged: true,
          generation: "daemon",
          revision: 1,
        }),
      };
      try {
        expect(queryClient.getQueryData(roleKey)).toEqual([]);
        if (transition === "refresh")
          await refreshAndApplyProvidersSnapshot({
            client,
            serverId,
            cwd: snapshot.cwd,
            providers: [provider],
            queryClient,
            cache,
          });
        else if (transition === "first-ready")
          await queryClient.fetchQuery({
            queryKey: key,
            queryFn: () =>
              fetchProvidersSnapshot({ client, serverId, cwd: snapshot.cwd, queryClient, cache }),
          });
        else
          await applyProvidersSnapshotUpdate({
            client,
            serverId,
            queryClient,
            cache,
            message: { type: "provider.snapshot.update", payload: ready },
          });
        await repaired;
        expect(queryClient.getQueryData(roleKey)).toEqual(roles);
        expect(readRoles).toHaveBeenCalledTimes(1);
        for (const other of untouched)
          expect(queryClient.getQueryState(other)?.isInvalidated).toBe(false);
        await applyProvidersSnapshotUpdate({
          client,
          serverId,
          queryClient,
          cache,
          message: { type: "provider.snapshot.update", payload: ready },
        });
        expect(readRoles).toHaveBeenCalledTimes(1);
      } finally {
        unsubscribe();
        queryClient.clear();
      }
    },
  );

  it("retires an unfinished first role read so a late empty response cannot replace the ready catalog", async () => {
    const queryClient = new QueryClient();
    const cache = createProviderSnapshotCache(createStorage());
    const serverId = "roles-in-flight";
    const provider = entries[0]!.provider;
    const roleKey = ["providerPresetFeatures", serverId, provider, snapshot.cwd];
    let releaseOld!: (value: AgentFeature[]) => void;
    const oldRead = new Promise<AgentFeature[]>((resolve) => {
      releaseOld = resolve;
    });
    const readRoles = vi.fn().mockReturnValueOnce(oldRead).mockResolvedValue(roles);
    const observer = new QueryObserver<AgentFeature[]>(queryClient, {
      queryKey: roleKey,
      queryFn: readRoles,
      staleTime: 60_000,
    });
    let markRepaired = () => {};
    const repaired = new Promise<void>((resolve) => {
      markRepaired = resolve;
    });
    const unsubscribe = observer.subscribe((result) => {
      if (result.data?.length && !result.isFetching) markRepaired();
    });
    try {
      expect(readRoles).toHaveBeenCalledTimes(1);
      await applyProvidersSnapshotUpdate({
        client: { getProvidersSnapshot: async () => snapshot },
        serverId,
        queryClient,
        cache,
        message: {
          type: "provider.snapshot.update",
          payload: { ...snapshot, entries, generation: "daemon", revision: 2, refreshing: [] },
        },
      });
      await repaired;
      releaseOld([]);
      await oldRead;
      expect(queryClient.getQueryData(roleKey)).toEqual(roles);
      expect(readRoles).toHaveBeenCalledTimes(2);
    } finally {
      releaseOld([]);
      unsubscribe();
      queryClient.clear();
    }
  });

  it("keeps a genuine no-role result stable across repeated unchanged ready snapshots", async () => {
    const queryClient = new QueryClient();
    const cache = createProviderSnapshotCache(createStorage());
    const serverId = "roles-unsupported";
    const provider = entries[0]!.provider;
    const readRoles = vi.fn(async (): Promise<AgentFeature[]> => []);
    const observer = new QueryObserver(queryClient, {
      queryKey: ["providerPresetFeatures", serverId, provider, snapshot.cwd],
      queryFn: readRoles,
      initialData: [] as AgentFeature[],
      staleTime: 60_000,
    });
    let markRead = () => {};
    const read = new Promise<void>((resolve) => {
      markRead = resolve;
    });
    const unsubscribe = observer.subscribe((result) => {
      if (result.isFetchedAfterMount && !result.isFetching) markRead();
    });
    const input = {
      client: { getProvidersSnapshot: async () => snapshot },
      serverId,
      queryClient,
      cache,
      message: {
        type: "provider.snapshot.update" as const,
        payload: { ...snapshot, entries, generation: "daemon", revision: 2, refreshing: [] },
      },
    };
    try {
      await applyProvidersSnapshotUpdate(input);
      await read;
      await applyProvidersSnapshotUpdate(input);
      await applyProvidersSnapshotUpdate(input);
      expect(readRoles).toHaveBeenCalledTimes(1);
      expect(observer.getCurrentResult().data).toEqual([]);
    } finally {
      unsubscribe();
      queryClient.clear();
    }
  });
});

describe("model catalog access", () => {
  it("retains the last good choices on discovery failure and replaces them after retry", async () => {
    const queryClient = new QueryClient();
    const cache = createProviderSnapshotCache(createStorage());
    const serverId = "catalog-failure";
    const key = providersSnapshotQueryKey(serverId, snapshot.cwd);
    const failed = {
      ...entries[0]!,
      status: "error" as const,
      error: "Discovery failed",
      models: [],
    };
    const changed = { ...entries[0]!, models: [{ ...entries[0]!.models![0]!, id: "new-model" }] };
    const responses = [
      snapshot,
      { ...snapshot, snapshotHash: "failure", compactSnapshot: compactProviderSnapshot([failed]) },
      {
        ...snapshot,
        snapshotHash: "recovered",
        compactSnapshot: compactProviderSnapshot([changed]),
      },
    ];
    const client = { getProvidersSnapshot: vi.fn(async () => responses.shift()!) };
    try {
      for (let index = 0; index < 3; index += 1) {
        const result = await fetchProvidersSnapshot({
          client,
          serverId,
          cwd: snapshot.cwd,
          queryClient,
          cache,
        });
        queryClient.setQueryData(key, result);
        if (index === 1) {
          expect(result.entries[0]).toMatchObject({
            status: "error",
            error: "Discovery failed",
            models: entries[0]!.models,
          });
          expect((await cache.read(serverId, snapshot.cwd))?.entries).toEqual(entries);
        }
        if (index === 2) expect(result.entries[0]?.models?.[0]?.id).toBe("new-model");
      }
    } finally {
      queryClient.clear();
      replaceProviderSnapshotIcons(serverId, []);
    }
  });

  it("shares concurrent refreshes and does not queue another while discovery is loading", async () => {
    const queryClient = new QueryClient();
    const serverId = "catalog-coalescing";
    const key = providersSnapshotQueryKey(serverId, snapshot.cwd);
    queryClient.setQueryData(key, { ...snapshot, entries });
    const { promise: gate, resolve: release } = Promise.withResolvers<void>();
    const refreshProvidersSnapshot = vi.fn(async () => {
      await gate;
      return { requestId: "refresh", acknowledged: true };
    });
    const client = {
      refreshProvidersSnapshot,
      getProvidersSnapshot: vi.fn(async () => ({ ...snapshot, entries })),
    };
    try {
      const first = refreshProvidersOnAccess({ client, serverId, cwd: snapshot.cwd, queryClient });
      const second = refreshProvidersOnAccess({ client, serverId, cwd: snapshot.cwd, queryClient });
      expect(refreshProvidersSnapshot).toHaveBeenCalledTimes(1);
      release();
      await Promise.all([first, second]);
      queryClient.setQueryData(key, {
        ...snapshot,
        entries: [{ ...entries[0]!, status: "loading" }],
        refreshing: [entries[0]!.provider],
      });
      await refreshProvidersOnAccess({ client, serverId, cwd: snapshot.cwd, queryClient });
      expect(refreshProvidersSnapshot).toHaveBeenCalledTimes(1);
    } finally {
      release();
      queryClient.clear();
      replaceProviderSnapshotIcons(serverId, []);
    }
  });
});

describe("full provider snapshots with content hashes", () => {
  it("rejects an uncached not-modified response rather than caching an empty catalog", async () => {
    const queryClient = new QueryClient();
    const getProvidersSnapshot = vi.fn().mockResolvedValue({
      requestId: "not-modified",
      entries: [],
      snapshotHash: "missing-hash",
      generatedAt: snapshot.generatedAt,
      notModified: true,
    });
    try {
      await expect(
        fetchProvidersSnapshot({
          client: { getProvidersSnapshot },
          serverId: "missing-body",
          cwd: null,
          queryClient,
          cache: createProviderSnapshotCache(createStorage()),
        }),
      ).rejects.toBeInstanceOf(ProviderSnapshotCacheMissError);
      expect(getProvidersSnapshot).toHaveBeenCalledTimes(1);
    } finally {
      queryClient.clear();
    }
  });

  it.each([
    { name: "loading", entries: [{ ...entries[0]!, status: "loading" as const, models: [] }] },
    { name: "ready", entries },
    { name: "empty", entries: [] },
  ])("loads and caches a $name Rust daemon response without another transfer", async (input) => {
    const cache = createProviderSnapshotCache(createStorage());
    const queryClient = new QueryClient();
    const serverId = `full-snapshot-${input.name}`;
    const full = {
      requestId: "full-response",
      cwd: snapshot.cwd,
      entries: input.entries,
      snapshotHash: `full-${input.name}`,
      generatedAt: snapshot.generatedAt,
      generation: "daemon",
      revision: 1,
      refreshing: input.name === "loading" ? [entries[0]!.provider] : [],
    };
    const getProvidersSnapshot = vi
      .fn()
      .mockResolvedValueOnce(full)
      .mockResolvedValueOnce({ ...full, entries: [], notModified: true });
    try {
      for (let read = 0; read < 2; read += 1) {
        const result = await fetchProvidersSnapshot({
          client: { getProvidersSnapshot },
          serverId,
          cwd: snapshot.cwd,
          queryClient,
          cache,
        });
        expect(result).toMatchObject({
          entries: input.entries,
          snapshotHash: full.snapshotHash,
          generation: full.generation,
          revision: full.revision,
          refreshing: full.refreshing,
        });
      }
      expect(getProvidersSnapshot.mock.calls).toEqual([
        [{ cwd: snapshot.cwd }],
        [{ cwd: snapshot.cwd, ifNoneMatch: full.snapshotHash }],
      ]);
      expect((await cache.read(serverId, snapshot.cwd))?.entries).toEqual(input.entries);
    } finally {
      queryClient.clear();
      replaceProviderSnapshotIcons(serverId, []);
    }
  });

  it.each(["full", "reference"])("applies a %s ready push after lazy discovery", async (kind) => {
    const cache = createProviderSnapshotCache(createStorage());
    const queryClient = new QueryClient();
    const serverId = `lazy-discovery-${kind}`;
    const full = {
      requestId: "ready-response",
      cwd: snapshot.cwd,
      entries,
      snapshotHash: "ready-hash",
      generatedAt: snapshot.generatedAt,
      generation: "daemon",
      revision: 2,
      refreshing: [],
    };
    const getProvidersSnapshot = vi.fn().mockResolvedValue(full);
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, snapshot.cwd), {
      ...full,
      entries: [{ ...entries[0]!, status: "loading", models: [] }],
      snapshotHash: "loading-hash",
      revision: 1,
      refreshing: [entries[0]!.provider],
    });
    try {
      await applyProvidersSnapshotUpdate({
        client: { getProvidersSnapshot },
        serverId,
        queryClient,
        cache,
        message: {
          type: "provider.snapshot.update",
          payload: { ...full, entries: kind === "full" ? entries : [] },
        },
      });
      expect(
        queryClient.getQueryData(providersSnapshotQueryKey(serverId, snapshot.cwd)),
      ).toMatchObject({ entries, snapshotHash: "ready-hash", revision: 2, refreshing: [] });
      expect(getProvidersSnapshot).toHaveBeenCalledTimes(kind === "full" ? 0 : 1);
      expect((await cache.read(serverId, snapshot.cwd))?.entries).toEqual(entries);
    } finally {
      queryClient.clear();
      replaceProviderSnapshotIcons(serverId, []);
    }
  });
});

describe("provider fetch with runtime abort signals", () => {
  it.each([
    ["React Native", NativeAbortController],
    ["default", AbortController],
  ])(
    "publishes ready models and persists the real compact snapshot with %s",
    async (_, Controller) => {
      const controller = new Controller();
      const cache = createProviderSnapshotCache(createStorage());
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false, gcTime: 0 } },
      });
      const serverId = "successful-provider-fetch";
      const queryKey = providersSnapshotQueryKey(serverId, snapshot.cwd);
      try {
        const result = await queryClient.fetchQuery({
          queryKey,
          queryFn: () =>
            fetchProvidersSnapshot({
              client: { getProvidersSnapshot: async () => snapshot },
              serverId,
              cwd: snapshot.cwd,
              queryClient,
              cache,
              signal: controller.signal,
            }),
        });
        expect(result.entries).toEqual(entries);
        expect(queryClient.getQueryData(queryKey)).toEqual(result);
        expect((await cache.read(serverId, snapshot.cwd))?.entries).toEqual(entries);
        expect(resolveProviderIconName("test-provider", serverId)).toEqual({
          kind: "svg",
          svg: entries[0].iconSvg,
        });
      } finally {
        queryClient.clear();
        replaceProviderSnapshotIcons(serverId, []);
      }
    },
  );
});

describe.each([
  ["React Native", NativeAbortController],
  ["default", AbortController],
])("provider fetch cancellation with %s", (_, Controller) => {
  it.each([
    { boundary: "response", abortOnWrite: 0, hasBody: false, hasReference: false },
    { boundary: "body persistence", abortOnWrite: 1, hasBody: true, hasReference: false },
    { boundary: "directory persistence", abortOnWrite: 2, hasBody: true, hasReference: true },
  ])(
    "does not publish after cancellation at $boundary",
    async ({ abortOnWrite, hasBody, hasReference }) => {
      const controller = new Controller();
      let writes = 0;
      const storage = createStorage(() => {
        writes += 1;
        if (writes === abortOnWrite) controller.abort();
      });
      const cache = createProviderSnapshotCache(storage);
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false, gcTime: 0 } },
      });
      const serverId = "cancelled-provider-fetch";
      const queryKey = providersSnapshotQueryKey(serverId, snapshot.cwd);
      const previous = {
        ...snapshot,
        entries: [{ ...entries[0], iconSvg: "previous-icon" }],
      };
      queryClient.setQueryData(queryKey, previous);
      replaceProviderSnapshotIcons(serverId, previous.entries);
      try {
        await expect(
          queryClient.fetchQuery({
            queryKey,
            queryFn: () =>
              fetchProvidersSnapshot({
                client: {
                  getProvidersSnapshot: async () => {
                    if (abortOnWrite === 0) controller.abort();
                    return snapshot;
                  },
                },
                serverId,
                cwd: snapshot.cwd,
                queryClient,
                cache,
                signal: controller.signal,
              }),
          }),
        ).rejects.toBeInstanceOf(CancelledError);
        expect(controller.signal.aborted).toBe(true);
        expect(queryClient.getQueryData(queryKey)).toEqual(previous);
        expect(resolveProviderIconName("test-provider", serverId)).toEqual({
          kind: "svg",
          svg: "previous-icon",
        });
        expect((await cache.readHash(serverId, "test-hash"))?.entries ?? null).toEqual(
          hasBody ? entries : null,
        );
        expect((await cache.read(serverId, snapshot.cwd))?.entries ?? null).toEqual(
          hasReference ? entries : null,
        );
      } finally {
        queryClient.clear();
        replaceProviderSnapshotIcons(serverId, []);
      }
    },
  );
});

it("keeps a replacement query when the cancelled native fetch finishes late", async () => {
  const controller = new NativeAbortController();
  const cache = createProviderSnapshotCache(createStorage());
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const serverId = "superseded-provider-fetch";
  const queryKey = providersSnapshotQueryKey(serverId, snapshot.cwd);
  let finishResponse = () => {};
  const responseReady = new Promise<void>((resolve) => {
    finishResponse = resolve;
  });
  let markStarted = () => {};
  const started = new Promise<void>((resolve) => {
    markStarted = resolve;
  });
  let oldFetch: Promise<GetProvidersSnapshotResponseMessage["payload"]> | undefined;
  try {
    const oldQuery = queryClient.fetchQuery({
      queryKey,
      queryFn: ({ signal }) => {
        signal.addEventListener("abort", () => controller.abort(), { once: true });
        oldFetch = fetchProvidersSnapshot({
          client: {
            getProvidersSnapshot: async () => {
              markStarted();
              await responseReady;
              return snapshot;
            },
          },
          serverId,
          cwd: snapshot.cwd,
          queryClient,
          cache,
          signal: controller.signal,
        });
        return oldFetch;
      },
    });
    const cancelledQuery = expect(oldQuery).rejects.toBeInstanceOf(CancelledError);
    await started;
    await queryClient.cancelQueries({ queryKey, exact: true });
    await cancelledQuery;
    expect(controller.signal.aborted).toBe(true);
    const replacementEntries = [{ ...entries[0], iconSvg: "replacement-icon", models: [] }];
    const replacement = await queryClient.fetchQuery({
      queryKey,
      queryFn: ({ signal }) =>
        fetchProvidersSnapshot({
          client: {
            getProvidersSnapshot: async () => ({
              ...snapshot,
              snapshotHash: "replacement-hash",
              compactSnapshot: compactProviderSnapshot(replacementEntries),
            }),
          },
          serverId,
          cwd: snapshot.cwd,
          queryClient,
          cache,
          signal,
        }),
    });
    finishResponse();
    await expect(oldFetch).rejects.toBeInstanceOf(CancelledError);
    expect(queryClient.getQueryData(queryKey)).toEqual(replacement);
    expect(replacement.entries).toEqual(replacementEntries);
    expect((await cache.read(serverId, snapshot.cwd))?.entries).toEqual(replacementEntries);
    expect(resolveProviderIconName("test-provider", serverId)).toEqual({
      kind: "svg",
      svg: "replacement-icon",
    });
  } finally {
    finishResponse();
    queryClient.clear();
    replaceProviderSnapshotIcons(serverId, []);
  }
});

describe("provider snapshot publication identity", () => {
  it("keeps the hash cache's canonical objects across a catalog change and later announcements", async () => {
    const cache = createProviderSnapshotCache(createStorage());
    const queryClient = new QueryClient();
    const serverId = "canonical-provider-snapshot";
    const client = {
      async getProvidersSnapshot() {
        throw new Error("Unexpected content fetch");
      },
    };
    const changed = {
      ...snapshot,
      snapshotHash: "next-hash",
      compactSnapshot: compactProviderSnapshot([{ ...entries[0]!, label: "Updated provider" }]),
    };
    try {
      for (const body of [snapshot, changed, changed]) {
        await applyProvidersSnapshotUpdate({
          serverId,
          queryClient,
          cache,
          client,
          message: { type: "provider.snapshot.update", payload: body },
        });
        const canonical = await cache.materialize(serverId, body);
        const published = queryClient.getQueryData<GetProvidersSnapshotResponseMessage["payload"]>(
          providersSnapshotQueryKey(serverId, body.cwd),
        )!;
        expect(published.entries).toBe(canonical.entries);
        expect(published.compactSnapshot).toBe(canonical.compactSnapshot);
        expect(published.entries[0]!.models).toBe(canonical.entries[0]!.models);
      }
    } finally {
      queryClient.clear();
    }
  });
});

describe("same-hash provider discovery completion", () => {
  it.each([false, true])(
    "refreshes role discovery with unchanged model content and explicit metadata = %s",
    async (explicitMetadata) => {
      const queryClient = new QueryClient();
      const cache = createProviderSnapshotCache(createStorage());
      const serverId = `same-hash-roles-${explicitMetadata}`;
      const provider = entries[0]!.provider;
      const oldTime = "2026-09-07T00:00:00.000Z";
      const newTime = "2026-09-07T00:01:00.000Z";
      const oldEntry = { ...entries[0]!, fetchedAt: oldTime };
      const compact = compactProviderSnapshot([oldEntry]);
      const previous = {
        ...snapshot,
        snapshotHash: "same-model-body",
        compactSnapshot: compact,
        entries: [oldEntry],
        generation: "daemon",
        revision: 1,
        refreshing: [],
      };
      await cache.write({
        serverId,
        cwd: snapshot.cwd,
        hash: previous.snapshotHash,
        generatedAt: snapshot.generatedAt,
        compactSnapshot: compact,
        fetchedAt: { [provider]: oldTime },
      });
      queryClient.setQueryData(providersSnapshotQueryKey(serverId, snapshot.cwd), previous);
      const roles: AgentFeature[] = [
        { type: "select", id: "modelRole.task", label: "Task", value: "native", options: [] },
      ];
      const roleKey = ["providerPresetFeatures", serverId, provider, snapshot.cwd];
      const readRoles = vi.fn(async () => roles);
      const observer = new QueryObserver(queryClient, {
        queryKey: roleKey,
        queryFn: readRoles,
        initialData: [] as AgentFeature[],
        staleTime: 60_000,
      });
      let markRepaired = () => {};
      const repaired = new Promise<void>((resolve) => {
        markRepaired = resolve;
      });
      const unsubscribe = observer.subscribe((result) => {
        if (result.data?.length && !result.isFetching) markRepaired();
      });
      // No intermediate loading event was delivered. Native legacy full pushes carry fresh
      // entry timestamps even when the directory's model body and content hash are unchanged.
      const terminal = {
        requestId: "same-hash-terminal",
        cwd: snapshot.cwd,
        generatedAt: snapshot.generatedAt,
        snapshotHash: previous.snapshotHash,
        entries: [{ ...oldEntry, fetchedAt: explicitMetadata ? oldTime : newTime }],
        generation: "daemon",
        revision: 2,
        refreshing: [],
        ...(explicitMetadata ? { fetchedAt: { [provider]: newTime } } : {}),
      };
      const input = {
        client: { getProvidersSnapshot: async () => terminal },
        serverId,
        queryClient,
        cache,
        message: { type: "provider.snapshot.update" as const, payload: terminal },
      };
      try {
        await applyProvidersSnapshotUpdate(input);
        await repaired;
        expect(readRoles).toHaveBeenCalledTimes(1);
        expect(queryClient.getQueryData(roleKey)).toEqual(roles);
        expect(
          queryClient.getQueryData<GetProvidersSnapshotResponseMessage["payload"]>(
            providersSnapshotQueryKey(serverId, snapshot.cwd),
          )?.entries[0]?.fetchedAt,
        ).toBe(newTime);
        // Only the directory association's timestamp changes; shared content and its hash stay.
        expect((await cache.readHash(serverId, previous.snapshotHash))?.compactSnapshot).toEqual(
          compact,
        );
        expect((await cache.read(serverId, snapshot.cwd))?.entries[0]?.fetchedAt).toBe(newTime);
        await applyProvidersSnapshotUpdate(input);
        expect(readRoles).toHaveBeenCalledTimes(1);
      } finally {
        unsubscribe();
        queryClient.clear();
      }
    },
  );
});

describe("legacy conditional catalog completion", () => {
  it("repairs ready role queries when only the same-generation revision advances, once per revision and directory", async () => {
    const queryClient = new QueryClient();
    const cache = createProviderSnapshotCache(createStorage());
    const serverId = "legacy-conditional-roles";
    const otherProvider = "second-ready-provider";
    const entry = { ...entries[0]!, fetchedAt: "2026-09-07T00:00:00.000Z" };
    const readyEntries = [entry, { ...entry, provider: otherProvider }];
    const compact = compactProviderSnapshot(readyEntries);
    const previous = {
      ...snapshot,
      entries: readyEntries,
      compactSnapshot: compact,
      snapshotHash: "conditional-same-body",
      generation: "daemon",
      revision: 1,
      refreshing: [],
    };
    await cache.write({
      serverId,
      cwd: snapshot.cwd,
      hash: previous.snapshotHash,
      generatedAt: snapshot.generatedAt,
      compactSnapshot: compact,
    });
    const snapshotKey = providersSnapshotQueryKey(serverId, snapshot.cwd);
    queryClient.setQueryData(snapshotKey, previous);
    const untouched = [
      ["providerPresetFeatures", "another-host", entry.provider, snapshot.cwd],
      ["providerPresetFeatures", serverId, entry.provider, "/another-repo"],
      ["provider-presets", serverId, entry.provider, snapshot.cwd],
    ];
    for (const key of untouched) queryClient.setQueryData(key, []);
    const subscriptions = readyEntries.map((candidate) => {
      const roles: AgentFeature[] = [
        { type: "select", id: "modelRole.task", label: "Task", value: "native", options: [] },
      ];
      const readRoles = vi.fn(async () => roles);
      const observer = new QueryObserver(queryClient, {
        queryKey: ["providerPresetFeatures", serverId, candidate.provider, snapshot.cwd],
        queryFn: readRoles,
        initialData: [] as AgentFeature[],
        staleTime: 60_000,
      });
      let markRepaired = () => {};
      const repaired = new Promise<void>((resolve) => {
        markRepaired = resolve;
      });
      const unsubscribe = observer.subscribe((result) => {
        if (result.data?.length && !result.isFetching) markRepaired();
      });
      return { readRoles, repaired, unsubscribe };
    });
    // This is the native conditional GET: unchanged content, no entries, no timestamp map,
    // no intervening loading push, but a newer discovery revision in the same daemon.
    const terminal = {
      requestId: "conditional-terminal",
      cwd: snapshot.cwd,
      generatedAt: snapshot.generatedAt,
      snapshotHash: previous.snapshotHash,
      notModified: true,
      entries: [],
      generation: "daemon",
      revision: 2,
      refreshing: [],
    };
    const getProvidersSnapshot = vi.fn(async () => terminal);
    const read = () =>
      queryClient.fetchQuery({
        queryKey: snapshotKey,
        queryFn: () =>
          fetchProvidersSnapshot({
            client: { getProvidersSnapshot },
            serverId,
            cwd: snapshot.cwd,
            queryClient,
            cache,
          }),
      });
    try {
      await read();
      await Promise.all(subscriptions.map((subscription) => subscription.repaired));
      for (const subscription of subscriptions)
        expect(subscription.readRoles).toHaveBeenCalledTimes(1);
      for (const key of untouched)
        expect(queryClient.getQueryState(key)?.isInvalidated).toBe(false);
      expect(getProvidersSnapshot).toHaveBeenLastCalledWith({
        cwd: snapshot.cwd,
        ifNoneMatch: previous.snapshotHash,
      });
      await read();
      await read();
      for (const subscription of subscriptions)
        expect(subscription.readRoles).toHaveBeenCalledTimes(1);
      expect((await cache.readHash(serverId, previous.snapshotHash))?.compactSnapshot).toEqual(
        compact,
      );
    } finally {
      for (const subscription of subscriptions) subscription.unsubscribe();
      queryClient.clear();
    }
  });
});
