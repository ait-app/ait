// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, test, vi } from "vitest";
import type { PropsWithChildren } from "react";
import fixture from "../../../../crates/provider/tests/fixtures/omp_commands.json";
import { useAgentAutocomplete } from "./use-agent-autocomplete";

vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => null,
  useHostRuntimeIsConnected: () => false,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: { host: { agents: new Map([["agent", { cwd: "/repo", provider: "omp" }]]) } },
    }),
}));
vi.mock("./use-agent-commands-query", () => ({
  useAgentCommandsQuery: () => ({
    commands: fixture.commands.map((command) => ({
      ...command,
      argumentHint: command.input?.hint ?? "",
      kind: command.source === "skill" ? "skill" : "command",
    })),
    isLoading: false,
    isError: false,
  }),
}));

afterEach(cleanup);

function setup(text: string) {
  const setUserInput = vi.fn();
  const onClientSlashCommand = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(
    ({ text }) =>
      useAgentAutocomplete({
        serverId: "host",
        agentId: "agent",
        userInput: text,
        cursorIndex: text.length,
        setUserInput,
        onClientSlashCommand,
        canExecuteClientSlashCommand: true,
      }),
    { initialProps: { text }, wrapper },
  );
  return { ...hook, setUserInput, onClientSlashCommand };
}

function key(text: string, key: string) {
  return {
    key,
    preventDefault: vi.fn(),
    input: { text, selection: { start: text.length, end: text.length } },
  };
}

test("OMP arguments display native labels and descriptions and Tab accepts the first candidate", () => {
  const text = "/advisor ";
  const { result, setUserInput } = setup(text);
  expect(result.current.isVisible).toBe(true);
  expect(result.current.options.map((option) => option.label).toReversed()).toEqual([
    "on",
    "off",
    "status",
    "dump",
    "configure",
  ]);
  expect(result.current.options.at(-1)?.description).toBe("Enable the advisor");
  const event = key(text, "Tab");
  act(() => {
    expect(result.current.onKeyPress(event)).toBe(true);
  });
  expect(event.preventDefault).toHaveBeenCalled();
  expect(setUserInput).toHaveBeenCalledWith("/advisor on ", { start: 12, end: 12 });
});

test("ArrowUp and Enter accept the selected native argument without submitting a prompt", () => {
  const text = "/adv o";
  const { result, setUserInput } = setup(text);
  act(() => {
    result.current.onKeyPress(key(text, "ArrowUp"));
  });
  act(() => {
    result.current.onKeyPress(key(text, "Enter"));
  });
  expect(setUserInput).toHaveBeenCalledWith("/adv off ", { start: 9, end: 9 });
});

test("a completed argument releases Enter to submit the native command", () => {
  const text = "/advisor on ";
  const { result } = setup(text);
  expect(result.current.isVisible).toBe(false);
  expect(result.current.onKeyPress(key(text, "Enter"))).toBe(false);
});

test("Escape dismisses OMP completions while preserving the draft", () => {
  const text = "/advisor ";
  const { result, setUserInput, rerender } = setup(text);
  act(() => {
    result.current.onKeyPress(key(text, "Escape"));
  });
  expect(result.current.isVisible).toBe(false);
  expect(setUserInput).not.toHaveBeenCalled();
  rerender({ text: "/advisor o" });
  expect(result.current.isVisible).toBe(true);
});

test("stale candidates cannot replace arguments for a different live command", () => {
  const { result, setUserInput } = setup("/advisor ");
  act(() =>
    result.current.onSelectOption(result.current.options.at(-1)!, {
      text: "/sample ",
      selection: { start: 8, end: 8 },
    }),
  );
  expect(setUserInput).not.toHaveBeenCalled();
});

test("accepting an argument in the middle of the prompt preserves its suffix and caret", () => {
  const { result, setUserInput } = setup("/advisor o");
  act(() =>
    result.current.onSelectOption(result.current.options.at(-1)!, {
      text: "/advisor off trailing",
      selection: { start: 10, end: 10 },
    }),
  );
  expect(setUserInput).toHaveBeenCalledWith("/advisor on trailing", { start: 12, end: 12 });
});

test("OMP's clear completion remains native and does not invoke the client's archive action", () => {
  const { result, setUserInput, onClientSlashCommand } = setup("/clear");
  expect(result.current.options).toEqual([
    expect.objectContaining({ label: "/clear", description: "Start a native session" }),
  ]);
  act(() => result.current.onSelectOption(result.current.options[0]));
  expect(setUserInput).toHaveBeenCalledWith("/clear ");
  expect(onClientSlashCommand).not.toHaveBeenCalled();
});

test("usage metadata accompanies the native subcommand description", () => {
  const { result } = setup("/sample ");
  expect(result.current.options[0]).toMatchObject({
    label: "run",
    description: "Run the action",
    detail: "<path>",
  });
});
