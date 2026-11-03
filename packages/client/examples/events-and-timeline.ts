import { createPaseoClient, type PaseoClient, type PaseoClientConfig } from "@ait/client";

export function createClient(config: PaseoClientConfig): PaseoClient {
  return createPaseoClient(config);
}

export async function subscribeToEvents(
  config: PaseoClientConfig,
  agentId: string,
  workspaceId: string,
): Promise<() => Promise<void>> {
  const client = createClient(config);
  await client.connect();

  const agents = await client.agents.list({ subscribe: {} });
  const workspaces = await client.workspaces.list({ subscribe: {} });

  const unsubscribeAgentUpdates = client.agents.subscribe((update) => {
    if (update.kind === "upsert" && update.agent.id === agentId) {
      void update.agent.status;
    }
  });

  const unsubscribeWorkspaceUpdates = client.workspaces.subscribe((update) => {
    if (update.kind === "upsert" && update.workspace.id === workspaceId) {
      void update.workspace.workspaceDirectory;
    }
  });

  const unsubscribeTimeline = client.agents.ref(agentId).timeline.subscribe((event) => {
    if (event.event.type === "subscription_restored") {
      console.log("Live subscription restored; request any missed history explicitly.");
    } else if (event.event.type === "error") {
      console.error(event.event.error);
    }
  });

  await unsubscribeTimeline.ready;

  return async () => {
    unsubscribeTimeline();
    unsubscribeWorkspaceUpdates();
    unsubscribeAgentUpdates();
    try {
      await Promise.all([agents.subscription.release(), workspaces.subscription.release()]);
    } finally {
      await client.close();
    }
  };
}

export async function refetchTimeline(config: PaseoClientConfig, agentId: string): Promise<number> {
  const client = createClient(config);

  try {
    await client.connect();

    const timeline = await client.agents.ref(agentId).timeline.refetch({
      direction: "before",
      limit: 50,
      projection: "projected",
    });

    return timeline.entries.length;
  } finally {
    await client.close();
  }
}
