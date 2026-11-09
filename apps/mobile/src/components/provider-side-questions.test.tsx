// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ProviderUiState } from "@ait/protocol/messages";
import { ProviderSideQuestions } from "./provider-side-questions";

const { runNativeAction } = vi.hoisted(() => ({ runNativeAction: vi.fn() }));
vi.mock("@/components/markdown/renderer", () => ({
  MarkdownRenderer: ({ text }: { text: string }) => <div data-testid="side-markdown">{text}</div>,
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => ({ runNativeAction }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    disabled,
    onPress,
    testID,
  }: {
    children: React.ReactNode;
    disabled?: boolean;
    onPress: () => void;
    testID?: string;
  }) => (
    <button disabled={disabled} onClick={onPress} data-testid={testID}>
      {children}
    </button>
  ),
}));
vi.mock("@/components/ui/status-badge", () => ({
  StatusBadge: ({ label }: { label: string }) => <span>{label}</span>,
}));

type State = NonNullable<ProviderUiState["sideQuestions"]>;
const topic = {
  id: "topic",
  question: "Native side question",
  createdAt: 1,
  truncated: false,
  turns: [
    {
      question: "Native side question",
      answer: "Side answer",
      status: "complete" as const,
      createdAt: 1,
      updatedAt: 2,
      error: null,
    },
  ],
};
function state(): State {
  return { records: [topic], error: null, omitted: 0, reveal: "open", runningId: null };
}
function view(value: State) {
  return <ProviderSideQuestions state={value} serverId="server" agentId="agent" disabled={false} />;
}
function questionInput() {
  if (!screen.queryByLabelText("providerControl.sideQuestions.question"))
    fireEvent.click(screen.getByTestId("side-question-new"));
  return screen.getByLabelText("providerControl.sideQuestions.question");
}
beforeEach(() => {
  runNativeAction.mockResolvedValue({ text: "Accepted", error: null });
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

it("side-question admission leaves the editor editable and preserves a newer draft", async () => {
  let complete!: (value: unknown) => void;
  runNativeAction.mockImplementation(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  );
  render(view(state()));
  fireEvent.change(questionInput(), {
    target: { value: "First question" },
  });
  fireEvent.click(screen.getByTestId("side-question-send"));
  expect(runNativeAction).toHaveBeenCalledWith("agent", "btw", { question: "First question" });
  fireEvent.change(questionInput(), {
    target: { value: "Newer draft" },
  });
  complete({ text: "Accepted", error: null });
  await waitFor(() => expect(questionInput()).toHaveProperty("value", "Newer draft"));
});

it("follow-ups use the native topic id and cancellation stays outside the main turn", async () => {
  const value = state();
  const mounted = render(view(value));
  fireEvent.click(screen.getByText("providerControl.sideQuestions.followUp"));
  fireEvent.change(questionInput(), {
    target: { value: "Follow-up question" },
  });
  fireEvent.click(screen.getByTestId("side-question-send"));
  await waitFor(() =>
    expect(runNativeAction).toHaveBeenCalledWith("agent", "btw", {
      question: "Follow-up question",
      recordId: "topic",
    }),
  );
  mounted.rerender(
    view({
      ...value,
      runningId: "topic",
      records: [{ ...topic, turns: [{ ...topic.turns[0]!, status: "running" }] }],
    }),
  );
  fireEvent.click(screen.getByText("providerControl.sideQuestions.cancel"));
  await waitFor(() =>
    expect(runNativeAction).toHaveBeenCalledWith("agent", "btw_cancel", { recordId: "topic" }),
  );
});

it("reconnect shows an orphaned running preview as interrupted and allows a new question", () => {
  render(
    view({
      ...state(),
      records: [{ ...topic, turns: [{ ...topic.turns[0]!, status: "running" }] }],
    }),
  );
  expect(screen.getByText("providerControl.sideQuestions.states.interrupted")).toBeTruthy();
  fireEvent.change(questionInput(), {
    target: { value: "New question" },
  });
  expect(screen.getByTestId("side-question-send")).toHaveProperty("disabled", false);
});

it("failed side admission preserves its draft and a streamed final answer updates only the topic", async () => {
  runNativeAction.mockRejectedValue(new Error("Side provider failed"));
  const mounted = render(view(state()));
  fireEvent.change(questionInput(), {
    target: { value: "Keep this" },
  });
  fireEvent.click(screen.getByTestId("side-question-send"));
  await waitFor(() => expect(screen.getByText("Side provider failed")).toBeTruthy());
  expect(questionInput()).toHaveProperty("value", "Keep this");
  mounted.rerender(
    view({
      ...state(),
      records: [
        {
          ...topic,
          turns: [{ ...topic.turns[0]!, answer: "Updated native answer", updatedAt: 3 }],
        },
      ],
    }),
  );
  expect(screen.getByText("Updated native answer")).toBeTruthy();
});

it("closing hides the panel locally without touching native history", () => {
  render(view(state()));
  expect(screen.getByTestId("provider-side-questions")).toBeTruthy();
  fireEvent.click(screen.getByTestId("side-question-close"));
  expect(screen.queryByTestId("provider-side-questions")).toBeNull();
  expect(runNativeAction).not.toHaveBeenCalled();
});

it("a fresh reveal reopens a closed panel", () => {
  const value = state();
  const mounted = render(view(value));
  fireEvent.click(screen.getByTestId("side-question-close"));
  expect(screen.queryByTestId("provider-side-questions")).toBeNull();
  mounted.rerender(view({ ...value, reveal: "topic:2" }));
  expect(screen.getByTestId("provider-side-questions")).toBeTruthy();
  expect(runNativeAction).not.toHaveBeenCalled();
});

it("a new running side question reopens a closed panel without cancelling it", () => {
  const value = state();
  const mounted = render(view(value));
  fireEvent.click(screen.getByTestId("side-question-close"));
  expect(screen.queryByTestId("provider-side-questions")).toBeNull();
  mounted.rerender(
    view({
      ...value,
      runningId: "topic",
      records: [{ ...topic, turns: [{ ...topic.turns[0]!, status: "running" }] }],
    }),
  );
  expect(screen.getByTestId("provider-side-questions")).toBeTruthy();
  expect(screen.getByText("providerControl.sideQuestions.closeWhileRunning")).toBeTruthy();
  fireEvent.click(screen.getByTestId("side-question-close"));
  expect(screen.queryByTestId("provider-side-questions")).toBeNull();
  expect(runNativeAction).not.toHaveBeenCalled();
});

it("the dismiss control leaves collapse and expansion untouched", () => {
  render(view(state()));
  expect(questionInput()).toBeTruthy();
  fireEvent.click(screen.getByTestId("side-question-toggle"));
  expect(screen.queryByLabelText("providerControl.sideQuestions.question")).toBeNull();
  expect(screen.getByTestId("side-question-close")).toBeTruthy();
  fireEvent.click(screen.getByTestId("side-question-toggle"));
  expect(questionInput()).toBeTruthy();
  fireEvent.click(screen.getByTestId("side-question-close"));
  expect(screen.queryByTestId("provider-side-questions")).toBeNull();
});

it("uses Markdown for answers, avoids repeating the initial question, and labels the empty editor", () => {
  render(view(state()));
  expect(screen.getAllByText(topic.question)).toHaveLength(1);
  expect(screen.getByTestId("side-markdown").textContent).toBe("Side answer");
  expect(screen.queryByLabelText("providerControl.sideQuestions.question")).toBeNull();
  fireEvent.click(screen.getByTestId("side-question-new"));
  expect(screen.getByPlaceholderText("providerControl.sideQuestions.newPlaceholder")).toBeTruthy();
  fireEvent.click(screen.getByText("providerControl.sideQuestions.followUp"));
  expect(
    screen.getByPlaceholderText("providerControl.sideQuestions.followUpPlaceholder"),
  ).toBeTruthy();
});

it("keeps cancel/follow-up outside the answer scroll and shows one selected topic", () => {
  const value = state();
  const other = {
    ...topic,
    id: "other",
    question: "Second topic",
    turns: [{ ...topic.turns[0]!, question: "Second topic", answer: "Long answer\n".repeat(100) }],
  };
  const mounted = render(view({ ...value, records: [topic, other], runningId: "other" }));
  const answerScroll = screen.getByTestId("side-question-answer-scroll-other");
  const cancel = screen.getByText("providerControl.sideQuestions.cancel").closest("button")!;
  expect(answerScroll.contains(cancel)).toBe(false);
  expect(screen.queryByTestId("side-question-topic-topic")).toBeNull();
  mounted.rerender(view({ ...value, records: [topic, other] }));
  fireEvent.click(screen.getByRole("button", { name: "Native side question" }));
  expect(screen.getByTestId("side-question-topic-topic")).toBeTruthy();
  expect(screen.queryByTestId("side-question-topic-other")).toBeNull();
  expect(
    screen
      .getByTestId("side-question-answer-scroll-topic")
      .contains(screen.getByText("providerControl.sideQuestions.followUp")),
  ).toBe(false);
});

it("opens a second editor only for an explicit new question or follow-up", () => {
  render(view(state()));
  expect(screen.queryByTestId("side-question-input")).toBeNull();
  expect(screen.getByTestId("side-markdown")).toBeTruthy();
  fireEvent.click(screen.getByText("providerControl.sideQuestions.followUp"));
  expect(
    screen.getByPlaceholderText("providerControl.sideQuestions.followUpPlaceholder"),
  ).toBeTruthy();
  fireEvent.click(screen.getByTestId("side-question-new"));
  expect(screen.getByPlaceholderText("providerControl.sideQuestions.newPlaceholder")).toBeTruthy();
  expect(runNativeAction).not.toHaveBeenCalled();
});
