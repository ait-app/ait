import { createPaseoClient, type PaseoClient, type PaseoClientConfig } from "@ait/client";

export function createClient(config: PaseoClientConfig): PaseoClient {
  return createPaseoClient(config);
}

export async function createCodexAgent(config: PaseoClientConfig, cwd: string): Promise<string> {
  const client = createClient(config);

  try {
    await client.connect();

    const agent = await client.agents.create({
      config: {
        provider: "codex/gpt-5.5",
        modeId: "full-access",
      },
      cwd,
      prompt: "Inspect this repository and summarize the next useful task.",
    });

    return agent.id;
  } finally {
    await client.close();
  }
}

export async function chooseProviderFromSnapshot(
  config: PaseoClientConfig,
  cwd: string,
): Promise<string> {
  const client = createClient(config);

  try {
    await client.connect();

    const snapshot = await client.providers.waitForReady({ cwd });
    const readyProvider = snapshot.entries.find((provider) => provider.status === "ready");
    const model =
      readyProvider?.models?.find((candidate) => candidate.isDefault) ?? readyProvider?.models?.[0];
    if (!readyProvider || !model) throw new Error("No provider model is ready");

    const agent = await client.agents.create({
      config: {
        provider: `${readyProvider.provider}/${model.id}`,
      },
      cwd,
      prompt: "Start with a quick repository map.",
    });

    return agent.id;
  } finally {
    await client.close();
  }
}

export async function runFollowUp(
  config: PaseoClientConfig,
  agentId: string,
): Promise<string | null> {
  const client = createClient(config);

  try {
    await client.connect();
    const result = await client.agents.ref(agentId).run("Summarize your progress and next step.");
    if (result.status !== "idle") throw new Error(result.error ?? result.status);
    return result.lastMessage;
  } finally {
    await client.close();
  }
}
