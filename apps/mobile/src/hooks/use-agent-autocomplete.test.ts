// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AutocompleteOption } from "@/components/ui/autocomplete";
import type { AgentSlashCommand } from "./use-agent-commands-query";
import { useAgentAutocomplete } from "./use-agent-autocomplete";

const facts = vi.hoisted(() => ({
  commands: [] as AgentSlashCommand[],
  loaded: true,
  native: false,
  supported: false,
}));
vi.mock("./use-agent-commands-query", () => ({
  useAgentCommandsQuery: () => ({
    readOwnership: () => (facts.loaded ? facts.commands : undefined),
    commands: facts.commands,
    ownership: facts.loaded ? facts.commands : undefined,
    isLoading: !facts.loaded,
    isError: false,
  }),
}));
vi.mock("./use-agent-command-completions-query", () => ({
  useAgentCommandCompletionsQuery: () => ({
    active: facts.native,
    result: { supported: facts.supported, items: [] },
    isLoading: false,
  }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => null,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/stores/session-store", () => ({ useSessionStore: () => "/repo" }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let root: Root | undefined;
let cache: QueryClient;
const execute = vi.fn();
const updateInput = vi.fn();
let result: {
  options: AutocompleteOption[];
  isVisible: boolean;
  onSelectOption: (option: AutocompleteOption) => void;
};
function Probe() {
  result = useAgentAutocomplete({
    userInput: "/",
    cursorIndex: 1,
    setUserInput: updateInput,
    serverId: "server",
    agentId: "agent",
    onClientSlashCommand: execute,
    canExecuteClientSlashCommand: true,
  });
  return null;
}
async function render() {
  if (!root) {
    root = createRoot(document.createElement("div"));
    cache = new QueryClient();
  }
  await act(async () => {
    root?.render(createElement(QueryClientProvider, { client: cache }, createElement(Probe)));
  });
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  cache?.clear();
  execute.mockClear();
  updateInput.mockClear();
  Object.assign(facts, { commands: [], loaded: true, native: false, supported: false });
});

describe("current command ownership in autocomplete", () => {
  it("retains hidden native ownership without advertising the refused builtin", async () => {
    facts.commands = [
      {
        name: "clear",
        source: "builtin",
        menuVisible: false,
        description: "native",
        argumentHint: "",
      },
    ];
    await render();
    expect(result.options.some((option) => option.id === "clear")).toBe(false);
  });

  it("withholds client actions until ownership loads, including a successful empty catalog", async () => {
    facts.loaded = false;
    await render();
    expect(result.options.some((option) => option.id === "clear")).toBe(false);
    facts.loaded = true;
    await render();
    const clear = result.options.find((option) => option.id === "clear")!;
    act(() => result.onSelectOption(clear));
    expect(execute).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "replace-agent-with-draft" }),
    );
  });

  it("rejects a previously visible client selection after native ownership changes", async () => {
    await render();
    const previous = result.options.find((option) => option.id === "clear")!;
    facts.loaded = false;
    act(() => result.onSelectOption(previous));
    expect(execute).not.toHaveBeenCalled();
    expect(updateInput).not.toHaveBeenCalled();
    facts.loaded = true;
    facts.commands = [
      {
        name: "reset",
        aliases: ["clear:"],
        source: "plugin",
        description: "native",
        argumentHint: "",
      },
    ];
    act(() => result.onSelectOption(previous));
    expect(execute).not.toHaveBeenCalled();
    expect(updateInput).not.toHaveBeenCalled();
    await render();
    act(() => result.onSelectOption(previous));
    expect(execute).not.toHaveBeenCalled();
    expect(result.options.some((option) => option.id === "clear")).toBe(false);
    expect(result.options.find((option) => option.id === "reset")?.detail).toBe("plugin");
  });

  it("hides supported empty native completion rather than falling back to catalog rows", async () => {
    facts.commands = [
      { name: "deploy", source: "project", description: "native", argumentHint: "[env]" },
    ];
    facts.native = true;
    facts.supported = true;
    await render();
    expect(result.isVisible).toBe(false);
    expect(result.options).toEqual([]);
    facts.supported = false;
    await render();
    expect(result.options.find((option) => option.id === "deploy")?.detail).toBe("project · [env]");
  });
});
