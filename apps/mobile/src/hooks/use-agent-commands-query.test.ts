import { describe, expect, it } from "vitest";
import {
  type AgentCommandsClient,
  type DraftCommandConfig,
  commandOwnershipForCatalog,
  AgentCommandSessionCatalogUnavailable,
  fetchAgentCommands,
} from "./use-agent-commands-query";

type ListCommands = AgentCommandsClient["listCommands"];
type ListCommandsResult = Awaited<ReturnType<ListCommands>>;

interface ListCommandsCall {
  agentId: string;
  draftConfig: DraftCommandConfig | undefined;
}

interface FakeAgentCommandsClient extends AgentCommandsClient {
  calls: ListCommandsCall[];
}

function createClient(response: ListCommandsResult): FakeAgentCommandsClient {
  const calls: ListCommandsCall[] = [];
  return {
    calls,
    listCommands: (async (options: Parameters<ListCommands>[0]) => {
      calls.push({ agentId: options.agentId, draftConfig: options.draftConfig });
      return response;
    }) as ListCommands,
  };
}

function commandsPayload(commands: ListCommandsResult["commands"]): ListCommandsResult {
  return {
    requestId: "req_commands",
    agentId: "",
    error: null,
    commands,
    ownershipApplicable: false,
    ownershipComplete: false,
  };
}

describe("fetchAgentCommands", () => {
  it("distinguishes old hosts, unsupported providers, and incomplete new catalogs", () => {
    expect(commandOwnershipForCatalog(undefined, undefined)).toEqual({ kind: "unavailable" });
    expect(commandOwnershipForCatalog(false, undefined)).toEqual({ kind: "legacy" });
    expect(commandOwnershipForCatalog(true, undefined)).toEqual({ kind: "unavailable" });
    expect(
      commandOwnershipForCatalog(true, {
        commands: [],
        ownershipComplete: false,
        ownershipApplicable: false,
      }),
    ).toEqual({ kind: "legacy" });
    expect(
      commandOwnershipForCatalog(true, {
        commands: [],
        ownershipComplete: false,
        ownershipApplicable: true,
      }),
    ).toEqual({ kind: "unavailable" });
    expect(commandOwnershipForCatalog(true, { commands: [], ownershipComplete: true })).toEqual({
      kind: "unavailable",
    });
    expect(
      commandOwnershipForCatalog(true, {
        commands: [],
        ownershipComplete: true,
        ownershipApplicable: true,
      }),
    ).toEqual({ kind: "complete", commands: [] });
  });
  it("loads commands for a draft composer without an agent id", async () => {
    const client = createClient(
      commandsPayload([{ name: "compact", description: "Compact context", argumentHint: "" }]),
    );

    const draftConfig: DraftCommandConfig = {
      provider: "opencode",
      cwd: "/repo",
      modeId: "build",
    };

    const commands = await fetchAgentCommands({ client, agentId: "", draftConfig });

    expect(commands).toEqual({
      commands: [{ name: "compact", description: "Compact context", argumentHint: "" }],
      ownershipComplete: false,
      ownershipApplicable: false,
    });
    expect(client.calls).toEqual([{ agentId: "", draftConfig }]);
  });

  it("passes the agent id when fetching commands for a running agent", async () => {
    const client = createClient(commandsPayload([]));

    await fetchAgentCommands({ client, agentId: "agent-1" });

    expect(client.calls).toEqual([{ agentId: "agent-1", draftConfig: undefined }]);
  });

  it("preserves complete ownership and refuses failed catalog responses", async () => {
    const complete = { ...commandsPayload([]), ownershipComplete: true, ownershipApplicable: true };
    expect(
      await fetchAgentCommands({ client: createClient(complete), agentId: "agent-1" }),
    ).toEqual({
      commands: [],
      ownershipComplete: true,
      ownershipApplicable: true,
    });
    const failure = { ...commandsPayload([]), error: "not available" };
    await expect(
      fetchAgentCommands({ client: createClient(failure), agentId: "agent-1" }),
    ).rejects.toThrow("not available");
  });

  it("recognizes only the v2 live-session catalog error as needing an open or resumed session", async () => {
    const response = {
      ...commandsPayload([]),
      ownershipApplicable: true,
      ownershipComplete: false,
      error: "Command catalog is unavailable",
    };
    await expect(
      fetchAgentCommands({ client: createClient(response), agentId: "agent-1" }),
    ).rejects.toBeInstanceOf(AgentCommandSessionCatalogUnavailable);
    const draft: DraftCommandConfig = { provider: "omp", cwd: "/repo" };
    await expect(
      fetchAgentCommands({ client: createClient(response), agentId: "", draftConfig: draft }),
    ).rejects.not.toBeInstanceOf(AgentCommandSessionCatalogUnavailable);
    await expect(
      fetchAgentCommands({
        client: createClient({ ...response, ownershipApplicable: false }),
        agentId: "agent-1",
      }),
    ).rejects.not.toBeInstanceOf(AgentCommandSessionCatalogUnavailable);
    await expect(
      fetchAgentCommands({
        client: createClient({ ...response, error: "another failure" }),
        agentId: "agent-1",
      }),
    ).rejects.not.toBeInstanceOf(AgentCommandSessionCatalogUnavailable);
  });
});
