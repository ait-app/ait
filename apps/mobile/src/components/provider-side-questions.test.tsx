// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { SideQuestionsState, SideTopic } from "@ait/protocol/messages";
import { BtwPanel, classifyBtwHistoryFailure } from "./provider-side-questions";
import { useBtwVisibilityStore } from "@/stores/btw-visibility-store";
import { useBtwPanelState } from "@/btw/use-panel";

const mocks = vi.hoisted(() => ({
  ask: vi.fn(),
  cancel: vi.fn(),
  history: vi.fn(),
  supported: vi.fn(),
  transfer: vi.fn(),
  clientReady: true,
  connected: true,
  focused: true,
  live: undefined as SideQuestionsState | undefined,
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeIsConnected: () => mocks.connected,
  useHostRuntimeClient: () =>
    mocks.clientReady
      ? {
          askSideQuestion: mocks.ask,
          cancelSideQuestion: mocks.cancel,
          listSideQuestions: mocks.history,
          supportsSideQuestions: mocks.supported,
        }
      : null,
}));
vi.mock("@/btw/use-transfer", () => ({ useTransferSideQuestion: () => mocks.transfer }));
vi.mock("@/composer/keyboard-scope", () => ({
  useComposerKeyboardScope: () => ({ isPaneFocused: mocks.focused }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (selector: (state: unknown) => unknown) =>
    selector({
      sessions: {
        server: {
          agents: new Map([
            [
              "agent",
              {
                title: "Main task",
                model: "selected-model",
                providerUi: { sideQuestions: mocks.live },
              },
            ],
          ]),
        },
      },
    }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/components/markdown/renderer", () => ({
  MarkdownRenderer: ({ text }: { text: string }) => <div>{text}</div>,
}));

const topic: SideTopic = {
  id: "topic",
  question: "First question",
  answer: "First answer",
  status: "complete",
  createdAt: 1,
  updatedAt: 2,
  leafId: null,
  strategy: "isolated",
  truncated: false,
};
const onOpenedChange = vi.fn();
const onRequestConsumed = vi.fn();
const panel = (request: { question: string; key: number } | null = null, opened = true) => (
  <BtwPanel
    serverId="server"
    agentId="agent"
    workspaceId="workspace"
    disabled={false}
    opened={opened}
    request={request ? { ...request, owner: "server:agent" } : null}
    onRequestConsumed={onRequestConsumed}
    onOpenedChange={onOpenedChange}
  />
);

/** The production controller remains mounted while fullscreen/mode changes remove the panel. */
function ComposerPanelHarness({
  mounted = true,
  agentId = "agent",
  question = "Side question",
}: {
  mounted?: boolean;
  agentId?: string;
  question?: string;
}) {
  const controls = useBtwPanelState({ serverId: "server", agentId });
  return (
    <>
      <button onClick={() => controls.open(question)}>Open BTW</button>
      {mounted ? (
        <BtwPanel
          key={agentId}
          serverId="server"
          agentId={agentId}
          workspaceId="workspace"
          disabled={false}
          opened={controls.opened}
          request={controls.request}
          onRequestConsumed={controls.consume}
          onOpenedChange={controls.onOpenedChange}
        />
      ) : null}
    </>
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  mocks.focused = true;
  mocks.clientReady = true;
  mocks.connected = true;
  mocks.live = undefined;
  mocks.history.mockResolvedValue({ records: [topic], omitted: 0 });
  mocks.ask.mockResolvedValue({ receipt: "accepted", recordId: "topic" });
  mocks.cancel.mockResolvedValue(true);
  mocks.supported.mockReturnValue(true);
  mocks.transfer.mockResolvedValue(undefined);
  useBtwVisibilityStore.setState({ hydrated: true, ended: [] });
  Element.prototype.scrollTo = vi.fn();
});
afterEach(cleanup);

it("follows up the selected topic and renders streamed native updates without touching the main writer", async () => {
  const view = render(panel());
  await screen.findByText("First answer");
  fireEvent.change(screen.getByTestId("side-question-input"), { target: { value: "Follow up" } });
  fireEvent.click(screen.getByTestId("side-question-ask"));
  await waitFor(() =>
    expect(mocks.ask).toHaveBeenCalledWith({
      agentId: "agent",
      question: "Follow up",
      recordId: "topic",
    }),
  );
  mocks.live = { records: [{ ...topic, answer: "Streamed update", updatedAt: 3 }], omitted: 0 };
  view.rerender(panel());
  await screen.findByText("Streamed update");
});

it.each([true, false])(
  "loads and selects preserved history when the client becomes ready after opened=%s",
  async (opened) => {
    mocks.clientReady = false;
    mocks.live = { records: [], reveal: "topic", omitted: 0 };
    const view = render(panel(null, opened));
    await screen.findByTestId("btw-window");
    expect(mocks.history).not.toHaveBeenCalled();
    mocks.clientReady = true;
    view.rerender(panel(null, opened));
    await screen.findByText("First answer");
    expect(mocks.history).toHaveBeenCalledTimes(1);
    expect(mocks.ask).not.toHaveBeenCalled();
  },
);

it("waits for a retained client to reconnect before consuming an open command", async () => {
  mocks.connected = false;
  const request = { question: "Side question", key: 1 };
  const view = render(panel(request));
  await screen.findByTestId("btw-window");
  expect(mocks.history).not.toHaveBeenCalled();
  expect(mocks.ask).not.toHaveBeenCalled();
  expect(onRequestConsumed).not.toHaveBeenCalled();
  mocks.connected = true;
  view.rerender(panel(request));
  await waitFor(() => expect(mocks.ask).toHaveBeenCalledTimes(1));
  expect(onRequestConsumed).toHaveBeenCalledWith(1);
});

it("opens /btw question as a new topic and submits it exactly once after rerenders", async () => {
  const view = render(panel({ question: "Side question", key: 1 }));
  await waitFor(() => expect(mocks.ask).toHaveBeenCalledTimes(1));
  expect(mocks.ask).toHaveBeenCalledWith({
    agentId: "agent",
    question: "Side question",
    recordId: undefined,
  });
  view.rerender(panel({ question: "Side question", key: 1 }));
  await act(async () => {});
  expect(mocks.ask).toHaveBeenCalledTimes(1);
});

it("preserves newer text typed while an earlier question is being admitted", async () => {
  const admission = Promise.withResolvers<{ receipt: string; recordId: string }>();
  mocks.ask.mockReturnValue(admission.promise);
  mocks.history.mockResolvedValue({ records: [], omitted: 0 });
  render(panel());
  const input = screen.getByTestId("side-question-input");
  await waitFor(() =>
    expect(screen.getByTestId("side-question-ask")).toHaveProperty("disabled", true),
  );
  fireEvent.change(input, { target: { value: "First question" } });
  fireEvent.click(screen.getByTestId("side-question-ask"));
  fireEvent.change(input, { target: { value: "Newer draft" } });
  await act(async () => admission.resolve({ receipt: "accepted", recordId: "new-topic" }));
  expect(input).toHaveProperty("value", "Newer draft");
});

it("keeps the window open when cancellation cannot be confirmed", async () => {
  mocks.live = { records: [{ ...topic, status: "running" }], runningId: "topic", omitted: 0 };
  mocks.history.mockResolvedValue(mocks.live);
  mocks.cancel.mockRejectedValue(new Error("Connection lost"));
  render(panel());
  await screen.findByText("First answer");
  fireEvent.click(screen.getByTestId("side-question-end"));
  await screen.findByText("Connection lost");
  expect(onOpenedChange).not.toHaveBeenCalled();
  expect(mocks.cancel).toHaveBeenCalledWith({ agentId: "agent", recordId: "topic" });
});

it("closes only the BTW window after a confirmed cancel and retains the native history", async () => {
  mocks.live = {
    records: [{ ...topic, status: "running" }],
    runningId: "topic",
    reveal: "topic",
    omitted: 0,
  };
  mocks.history.mockResolvedValue(mocks.live);
  render(panel());
  await screen.findByText("First answer");
  fireEvent.click(screen.getByTestId("side-question-end"));
  await waitFor(() => expect(onOpenedChange).toHaveBeenCalledWith(false));
  expect(useBtwVisibilityStore.getState().ended).toContainEqual(["server:agent", "topic"]);
  expect(mocks.transfer).not.toHaveBeenCalled();
});

it("expands the selected multi-turn conversation and closes only after transfer succeeds", async () => {
  mocks.history.mockResolvedValue({
    records: [
      {
        ...topic,
        followUps: [
          {
            question: "Next",
            answer: "Next answer",
            status: "complete",
            createdAt: 3,
            updatedAt: 4,
          },
        ],
      },
    ],
    omitted: 0,
  });
  render(panel());
  await screen.findByText("Next answer");
  fireEvent.click(screen.getByTestId("side-question-expand"));
  await waitFor(() => expect(mocks.transfer).toHaveBeenCalledWith("topic"));
  expect(onOpenedChange).toHaveBeenCalledWith(false);
  expect(mocks.ask).not.toHaveBeenCalled();
});

it("removes its portal when the source pane is no longer focused", async () => {
  const view = render(panel());
  await screen.findByText("First answer");
  mocks.focused = false;
  view.rerender(panel());
  expect(screen.queryByTestId("btw-window")).toBeNull();
});

it("keeps transport errors retryable on a capable host", () => {
  expect(classifyBtwHistoryFailure(new Error("offline"), true)).toEqual({
    kind: "retryable",
    message: "offline",
  });
  expect(classifyBtwHistoryFailure(new Error("old host"), false)).toEqual({ kind: "unsupported" });
});

const rpcError = (code: string, requestType: string, message: string) =>
  Object.assign(new Error(`${message} requestType=${requestType} code=${code}`), {
    name: "DaemonRpcError",
    code,
    requestType,
    requestId: "rpc-request",
  });

it.each(["unsupported_capability", "not_implemented"])(
  "recognizes a real %s RPC response as an unsupported host",
  async (code) => {
    const error = rpcError(
      code,
      "agent.side_question.history.request",
      "Method is not implemented yet",
    );
    expect(classifyBtwHistoryFailure(error, true)).toEqual({ kind: "unsupported" });
    mocks.history.mockRejectedValue(error);
    render(panel());
    await screen.findByText("btw.unsupportedHost");
    expect(screen.queryByText(/requestType=|code=/)).toBeNull();
  },
);

it("keeps a readable retryable RPC reason without exposing request metadata", () => {
  expect(
    classifyBtwHistoryFailure(
      rpcError("connection_lost", "agent.side_question.history.request", "Connection lost"),
      true,
    ),
  ).toEqual({ kind: "retryable", message: "Connection lost" });
});

it("keeps an unsupported ask dismissible and its draft intact without exposing RPC metadata", async () => {
  mocks.ask.mockRejectedValue(
    rpcError("not_implemented", "agent.side_question.ask.request", "Method is not implemented yet"),
  );
  render(panel({ question: "Side question", key: 1 }));
  await screen.findByText("btw.unsupportedHost");
  expect(screen.getByTestId("side-question-input")).toHaveProperty("value", "Side question");
  expect(screen.queryByText(/requestType=|code=/)).toBeNull();
  expect(screen.getByTestId("side-question-end")).toHaveProperty("disabled", false);
  fireEvent.click(screen.getByTestId("side-question-end"));
  expect(onOpenedChange).toHaveBeenCalledWith(false);
  expect(mocks.ask).toHaveBeenCalledTimes(1);
});

it("does not resend an uncertain ask and waits for durable reconciliation before unlocking input", async () => {
  mocks.history.mockResolvedValue({ records: [], omitted: 0 });
  mocks.ask.mockResolvedValue({ receipt: "uncertain" });
  render(panel());
  await act(async () => {});
  fireEvent.change(screen.getByTestId("side-question-input"), { target: { value: "Question" } });
  fireEvent.click(screen.getByTestId("side-question-ask"));
  await screen.findByText("btw.uncertain");
  expect(screen.getByTestId("side-question-ask")).toHaveProperty("disabled", true);
  expect(screen.getByTestId("side-question-end")).toHaveProperty("disabled", true);
  mocks.history.mockResolvedValue({ records: [], omitted: 0, pendingAcceptance: true });
  fireEvent.click(screen.getByText("common.actions.retry"));
  await act(async () => {});
  fireEvent.change(screen.getByTestId("side-question-input"), {
    target: { value: "Next question" },
  });
  expect(screen.getByTestId("side-question-ask")).toHaveProperty("disabled", true);
  expect(mocks.ask).toHaveBeenCalledTimes(1);
  mocks.history.mockResolvedValue({ records: [], omitted: 0, pendingAcceptance: false });
  fireEvent.click(screen.getByText("common.actions.retry"));
  await waitFor(() =>
    expect(screen.getByTestId("side-question-ask")).toHaveProperty("disabled", false),
  );
  expect(mocks.ask).toHaveBeenCalledTimes(1);
});

it("shows the provider's specific unavailable reason without enabling a fake question path", async () => {
  mocks.history.mockResolvedValue({
    records: [],
    omitted: 0,
    available: false,
    unavailableReason: "Native side conversations are not exposed by this provider.",
  });
  render(panel());
  await screen.findByText("Native side conversations are not exposed by this provider.");
  expect(screen.getByTestId("side-question-ask")).toHaveProperty("disabled", true);
  expect(mocks.ask).not.toHaveBeenCalled();
});

it("keeps an old host's window dismissible without attempting an unsupported ask", async () => {
  mocks.supported.mockReturnValue(false);
  mocks.history.mockRejectedValue(new Error("UnsupportedCapability"));
  render(panel({ question: "Side question", key: 1 }));
  await screen.findByText("btw.unsupportedHost");
  expect(mocks.ask).not.toHaveBeenCalled();
  expect(screen.getByTestId("side-question-end")).toHaveProperty("disabled", false);
  fireEvent.click(screen.getByTestId("side-question-end"));
  expect(onOpenedChange).toHaveBeenCalledWith(false);
});

it("does not replay an admitted command after the panel is really unmounted and remounted", async () => {
  const admission = Promise.withResolvers<{ receipt: string; recordId: string }>();
  mocks.ask.mockReturnValue(admission.promise);
  const view = render(<ComposerPanelHarness />);
  fireEvent.click(screen.getByText("Open BTW"));
  await waitFor(() => expect(mocks.ask).toHaveBeenCalledTimes(1));
  view.rerender(<ComposerPanelHarness mounted={false} />);
  expect(screen.queryByTestId("btw-window")).toBeNull();
  view.rerender(<ComposerPanelHarness />);
  await screen.findByTestId("btw-window");
  await act(async () => admission.resolve({ receipt: "accepted", recordId: "topic" }));
  expect(mocks.ask).toHaveBeenCalledTimes(1);
});

it("drops a pending command when its source changes and never forwards it to another agent", async () => {
  const view = render(<ComposerPanelHarness mounted={false} />);
  fireEvent.click(screen.getByText("Open BTW"));
  view.rerender(<ComposerPanelHarness agentId="different-agent" />);
  await act(async () => {});
  view.rerender(<ComposerPanelHarness />);
  await act(async () => {});
  expect(mocks.ask).not.toHaveBeenCalled();
  expect(screen.queryByTestId("btw-window")).toBeNull();
});

it("keeps a closed history-first window closed when its live reveal arrives later", async () => {
  mocks.history.mockResolvedValue({ records: [topic], reveal: "topic", omitted: 0 });
  const view = render(<ComposerPanelHarness question="" />);
  fireEvent.click(screen.getByText("Open BTW"));
  await screen.findByText("First answer");
  await act(async () => {});
  fireEvent.click(screen.getByTestId("side-question-end"));
  await waitFor(() => expect(screen.queryByTestId("btw-window")).toBeNull());
  mocks.live = { records: [topic], reveal: "topic", omitted: 0 };
  view.rerender(<ComposerPanelHarness question="" />);
  await act(async () => {});
  expect(useBtwVisibilityStore.getState().ended).toContainEqual(["server:agent", "topic"]);
  expect(screen.queryByTestId("btw-window")).toBeNull();
});

it("can expand preserved history when its runner is unavailable, without allowing another ask", async () => {
  mocks.history.mockResolvedValue({
    records: [topic],
    omitted: 0,
    available: false,
    unavailableReason: "Native side conversations are unavailable.",
  });
  render(panel());
  await screen.findByText("First answer");
  expect(screen.getByTestId("side-question-ask")).toHaveProperty("disabled", true);
  expect(screen.getByTestId("side-question-expand")).toHaveProperty("disabled", false);
  fireEvent.click(screen.getByTestId("side-question-expand"));
  await waitFor(() => expect(mocks.transfer).toHaveBeenCalledWith("topic"));
  expect(onOpenedChange).toHaveBeenCalledWith(false);
  expect(mocks.ask).not.toHaveBeenCalled();
});

it("keeps an unknown history closed when its first history and live reveal arrive after dismissal", async () => {
  const history = Promise.withResolvers<SideQuestionsState>();
  mocks.history.mockReturnValue(history.promise);
  const view = render(<ComposerPanelHarness question="" />);
  fireEvent.click(screen.getByText("Open BTW"));
  await screen.findByTestId("btw-window");
  fireEvent.click(screen.getByTestId("side-question-end"));
  await waitFor(() => expect(screen.queryByTestId("btw-window")).toBeNull());
  expect(useBtwVisibilityStore.getState().ended).toContainEqual(["server:agent", "*"]);
  await act(async () => history.resolve({ records: [topic], reveal: "topic", omitted: 0 }));
  mocks.live = { records: [topic], reveal: "topic", omitted: 0 };
  view.rerender(<ComposerPanelHarness question="" />);
  await act(async () => {});
  expect(screen.queryByTestId("btw-window")).toBeNull();
  fireEvent.click(screen.getByText("Open BTW"));
  await screen.findByText("First answer");
  expect(mocks.ask).not.toHaveBeenCalled();
});
