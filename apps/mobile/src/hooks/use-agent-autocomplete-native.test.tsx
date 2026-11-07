// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, test, vi } from "vitest";
import type { PropsWithChildren } from "react";
import { useAgentAutocomplete } from "./use-agent-autocomplete";
import { applyNativeCommandCompletion } from "@/utils/native-command-completion";

const mocks = vi.hoisted(() => ({
  completeCommands: vi.fn(),
  commandsLoading: false,
  completionSupported: true,
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => mocks,
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/components/retained-panel", () => ({
  useRetainedPanelActive: () => true,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (select: (state: unknown) => unknown) =>
    select({
      sessions: {
        host: {
          serverInfo: { features: { agentCommandCompletion: mocks.completionSupported } },
          agents: new Map([["agent", { cwd: "/repo", provider: "omp" }]]),
        },
      },
    }),
}));
vi.mock("./use-agent-commands-query", () => ({
  useAgentCommandsQuery: () => ({
    commands: [
      {
        name: "catalog-fallback",
        description: "Fallback only",
        argumentHint: "",
      },
    ],
    isLoading: mocks.commandsLoading,
    isError: false,
  }),
}));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
  mocks.commandsLoading = false;
  mocks.completionSupported = true;
});

const candidate = {
  id: "native",
  label: "second",
  description: "Native second level",
  insertText: "second ",
  replaceRange: { start: 14, end: 15 },
  cursor: 21,
  triggerCompletion: true,
};
function setup(text = "/nested first s suffix", cursor = 15) {
  const setUserInput = vi.fn();
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(
    ({ text, cursor }) =>
      useAgentAutocomplete({
        serverId: "host",
        agentId: "agent",
        userInput: text,
        cursorIndex: cursor,
        setUserInput,
      }),
    { initialProps: { text, cursor }, wrapper },
  );
  return { ...hook, setUserInput };
}

test("queries the full native input and accepts deeper levels with exact suffix and caret", async () => {
  mocks.completeCommands.mockResolvedValue({
    supported: true,
    items: [candidate],
  });
  const { result, setUserInput } = setup();
  await waitFor(() => expect(result.current.options).toHaveLength(1));
  expect(mocks.completeCommands).toHaveBeenCalledWith(
    expect.objectContaining({
      text: "/nested first s suffix",
      cursor: 15,
    }),
  );
  expect(result.current.options[0]).toMatchObject({
    label: "second",
    description: "Native second level",
  });
  act(() => result.current.onSelectOption(result.current.options[0]));
  expect(setUserInput).toHaveBeenCalledWith("/nested first second  suffix", {
    start: 21,
    end: 21,
  });
});

test("native lookup waits for catalog discovery instead of starting a second draft probe", async () => {
  mocks.commandsLoading = true;
  mocks.completeCommands.mockResolvedValue({ supported: true, items: [candidate] });
  const { result, rerender } = setup();
  expect(mocks.completeCommands).not.toHaveBeenCalled();
  mocks.commandsLoading = false;
  rerender({ text: "/nested first s suffix", cursor: 15 });
  await waitFor(() => expect(result.current.options).toHaveLength(1));
  expect(mocks.completeCommands).toHaveBeenCalledTimes(1);
});

test("native empty results remain authoritative and release Enter for command execution", async () => {
  mocks.completeCommands.mockResolvedValue({ supported: true, items: [] });
  const text = "/nested first second ";
  const { result } = setup(text, text.length);
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  expect(result.current.options).toEqual([]);
  expect(result.current.isVisible).toBe(false);
  expect(
    result.current.onKeyPress({
      key: "Enter",
      preventDefault: vi.fn(),
      input: {
        text,
        selection: { start: text.length, end: text.length },
      },
    }),
  ).toBe(false);
});

test("pending completion keeps Tab focus and Enter from submitting, and Escape dismisses it", async () => {
  mocks.completeCommands.mockReturnValue(new Promise(() => {}));
  const text = "/nested first ";
  const { result, setUserInput } = setup(text, text.length);
  await waitFor(() => expect(mocks.completeCommands).toHaveBeenCalled());
  expect(result.current.isVisible).toBe(true);
  expect(result.current.isLoading).toBe(true);
  const input = { text, selection: { start: text.length, end: text.length } };
  for (const key of ["Tab", "Enter"]) {
    const preventDefault = vi.fn();
    expect(result.current.onKeyPress({ key, preventDefault, input })).toBe(true);
    expect(preventDefault).toHaveBeenCalled();
  }
  act(() => {
    expect(result.current.onKeyPress({ key: "Escape", preventDefault: vi.fn(), input })).toBe(true);
    expect(result.current.onKeyPress({ key: "Enter", preventDefault: vi.fn(), input })).toBe(false);
  });
  expect(result.current.isVisible).toBe(false);
  expect(result.current.onKeyPress({ key: "Enter", preventDefault: vi.fn(), input })).toBe(false);
  expect(setUserInput).not.toHaveBeenCalled();
});

test("a key received before the new editor state renders cannot accept stale completion", async () => {
  mocks.completeCommands.mockResolvedValue({ supported: true, items: [candidate] });
  const { result, setUserInput } = setup();
  await waitFor(() => expect(result.current.options).toHaveLength(1));
  const text = "/nested first se suffix";
  const preventDefault = vi.fn();
  expect(
    result.current.onKeyPress({
      key: "Tab",
      preventDefault,
      input: { text, selection: { start: 16, end: 16 } },
    }),
  ).toBe(true);
  expect(preventDefault).toHaveBeenCalled();
  expect(setUserInput).not.toHaveBeenCalled();
});

test("only unsupported native capability permits the static catalog fallback", async () => {
  mocks.completeCommands.mockResolvedValue({ supported: false, items: [] });
  const { result } = setup("/", 1);
  await waitFor(() => expect(result.current.options[0]?.label).toBe("/catalog-fallback"));
});

test("native errors remain visible without suggesting fabricated catalog results", async () => {
  mocks.completeCommands.mockRejectedValue(new Error("Native completion failed"));
  const { result } = setup("/", 1);
  await waitFor(() => expect(result.current.errorMessage).toBe("Native completion failed"));
  expect(result.current.options).toEqual([]);
});

test("old native candidates cannot modify changed text, a moved caret or a selection", async () => {
  mocks.completeCommands.mockResolvedValue({
    supported: true,
    items: [candidate],
  });
  const { result, setUserInput, rerender } = setup();
  await waitFor(() => expect(result.current.options).toHaveLength(1));
  const old = result.current.options[0];
  for (const [text, start, end] of [
    ["/other", 6, 6],
    ["/nested first s suffix", 14, 14],
    ["/nested first s suffix", 15, 16],
  ] as const) {
    act(() =>
      result.current.onSelectOption(old, {
        text,
        selection: { start, end },
      }),
    );
  }
  expect(setUserInput).not.toHaveBeenCalled();
  rerender({ text: "/nested first second ", cursor: 21 });
  expect(result.current.options).toEqual([]);
});

test("native edits reject malformed UTF-16 boundaries", () => {
  expect(
    applyNativeCommandCompletion("😀", {
      ...candidate,
      replaceRange: { start: 1, end: 2 },
    }),
  ).toBeNull();
  expect(
    applyNativeCommandCompletion("😀", {
      ...candidate,
      replaceRange: { start: 0, end: 2 },
      insertText: "😀",
      cursor: 1,
    }),
  ).toBeNull();
});

test("an older daemon without the completion method keeps the static catalog", async () => {
  mocks.completionSupported = false;
  const { result } = setup("/cat", 4);
  await waitFor(() =>
    expect(result.current.options.some((option) => option.label === "/catalog-fallback")).toBe(
      true,
    ),
  );
  expect(mocks.completeCommands).not.toHaveBeenCalled();
});

test("not_implemented falls back while advertised native errors remain visible", async () => {
  mocks.completeCommands.mockRejectedValue(
    Object.assign(new Error("Method is not implemented"), { code: "not_implemented" }),
  );
  const { result } = setup("/cat", 4);
  await waitFor(() =>
    expect(result.current.options.some((option) => option.label === "/catalog-fallback")).toBe(
      true,
    ),
  );
  expect(result.current.errorMessage).toBeFalsy();
});
