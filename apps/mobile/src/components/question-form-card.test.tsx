// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PendingPermission } from "@/types/shared";
import { AgentPermissionRequestPayloadSchema } from "@ait/protocol/messages";
import { QuestionFormCard } from "./question-form-card";
import ompQuestion from "../../../../crates/provider/tests/fixtures/omp_question.json";
import ompDialogs from "../../../../crates/provider/tests/fixtures/omp_dialogs.json";
import { parseQuestionFormQuestions } from "./question-form-card-core";

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/components/ui/text-input", () => ({
  EditingTextInput: ({
    initialValue,
    onChangeText,
    accessibilityLabel,
    editable,
    multiline,
    secureTextEntry,
  }: {
    initialValue: string;
    onChangeText: (text: string) => void;
    accessibilityLabel: string;
    editable: boolean;
    multiline?: boolean;
    secureTextEntry?: boolean;
  }) =>
    multiline ? (
      <textarea
        aria-label={accessibilityLabel}
        value={initialValue}
        disabled={!editable}
        onChange={(event) => onChangeText(event.target.value)}
      />
    ) : (
      <input
        aria-label={accessibilityLabel}
        type={secureTextEntry ? "password" : "text"}
        value={initialValue}
        disabled={!editable}
        onChange={(event) => onChangeText(event.target.value)}
      />
    ),
}));

const permission: PendingPermission = {
  key: "agent-a:question-a",
  agentId: "agent-a",
  request: {
    id: "question-a",
    provider: "codex",
    name: "request_user_input",
    kind: "question",
    input: {
      questions: [
        {
          header: "Choice",
          question: "Choose a runtime",
          options: [{ label: "Rust" }, { label: "Python" }],
          allowOther: true,
        },
      ],
    },
  },
};

describe("question response recovery", () => {
  beforeEach(() => vi.stubGlobal("React", React));
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it.each(["dismiss", "submit"] as const)(
    "restores %s and preserves the answer after a failed attempt",
    (action) => {
      const onRespond = vi.fn();
      const ui = (isResponding: boolean) => (
        <QuestionFormCard
          permission={permission}
          onRespond={onRespond}
          isResponding={isResponding}
        />
      );
      const { rerender } = render(ui(false));
      fireEvent.change(screen.getByRole("textbox", { name: "Choose a runtime" }), {
        target: { value: "My existing answer" },
      });
      const button = screen.getByTestId(
        action === "dismiss" ? "question-form-dismiss" : "question-form-primary-action",
      );
      const label = button.textContent ?? "";
      expect(label).not.toBe("");
      fireEvent.click(button);
      expect(onRespond).toHaveBeenCalledOnce();
      rerender(ui(true));
      expect(screen.getByRole("progressbar")).toBeTruthy();
      expect(button.getAttribute("aria-disabled")).toBe("true");
      expect(button.textContent).not.toContain(label);
      rerender(ui(false));
      expect(screen.queryByRole("progressbar")).toBeNull();
      expect(button.textContent).toContain(label);
      expect(button.getAttribute("aria-disabled")).not.toBe("true");
      expect(
        (screen.getByRole("textbox", { name: "Choose a runtime" }) as HTMLInputElement).value,
      ).toBe("My existing answer");
      fireEvent.click(button);
      expect(onRespond).toHaveBeenCalledTimes(2);
    },
  );

  it("does not issue another dismissal while the response is pending", () => {
    const onRespond = vi.fn();
    render(<QuestionFormCard permission={permission} onRespond={onRespond} isResponding />);
    fireEvent.click(screen.getByTestId("question-form-dismiss"));
    expect(onRespond).not.toHaveBeenCalled();
  });
  it("shows the failed request inline while preserving answers for retry", () => {
    const onRespond = vi.fn();
    const { rerender } = render(
      <QuestionFormCard permission={permission} onRespond={onRespond} isResponding={false} />,
    );
    fireEvent.change(screen.getByRole("textbox", { name: "Choose a runtime" }), {
      target: { value: "Keep my answer" },
    });
    rerender(
      <QuestionFormCard
        permission={permission}
        onRespond={onRespond}
        isResponding={false}
        responseError="Host disconnected"
      />,
    );
    expect(screen.getByRole("alert").textContent).toBe("Host disconnected");
    fireEvent.click(screen.getByTestId("question-form-primary-action"));
    expect(onRespond).toHaveBeenCalledOnce();
    expect(
      (screen.getByRole("textbox", { name: "Choose a runtime" }) as HTMLInputElement).value,
    ).toBe("Keep my answer");
  });

  it("renders the OMP wire question and submits native answer keys and multiple choices", () => {
    const fixture = ompQuestion;
    const onRespond = vi.fn();
    render(
      <QuestionFormCard
        permission={{
          key: "agent:ask-1",
          agentId: "agent",
          request: AgentPermissionRequestPayloadSchema.parse(fixture.request),
        }}
        onRespond={onRespond}
        isResponding={false}
      />,
    );

    expect(screen.getByText("Which database?")).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "A,B" }));
    expect(screen.getByText("Which features?")).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "Auth" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Search" }));
    fireEvent.click(screen.getByTestId("question-form-primary-action"));

    expect(onRespond).toHaveBeenCalledExactlyOnceWith({
      ...fixture.response,
      updatedInput: { ...fixture.request.input, ...fixture.response.updatedInput },
    });
  });

  it.each(
    ompDialogs.filter((fixture) => ["select", "input", "editor"].includes(fixture.native.method)),
  )("renders OMP $name and returns the exact native answer", (fixture) => {
    const request = AgentPermissionRequestPayloadSchema.parse(fixture.request);
    const question = parseQuestionFormQuestions(request.input)![0];
    const answer = fixture.nativeResponse.value!;
    const onRespond = vi.fn();
    render(
      <QuestionFormCard
        permission={{ key: `agent:${request.id}`, agentId: "agent", request }}
        onRespond={onRespond}
        isResponding={false}
      />,
    );

    expect(screen.getByText(question.question)).toBeTruthy();
    if (fixture.native.method === "select") {
      fireEvent.click(screen.getByRole("radio", { name: answer }));
    } else {
      const input = question.secret
        ? screen.getByLabelText(question.question)
        : screen.getByRole("textbox", { name: question.question });
      fireEvent.change(input, { target: { value: answer } });
    }
    fireEvent.click(screen.getByTestId("question-form-primary-action"));

    expect(onRespond).toHaveBeenCalledExactlyOnceWith({
      ...fixture.response,
      updatedInput: { ...request.input, ...fixture.response.updatedInput },
    });
  });

  it("masks secret input and submits the verbatim value", () => {
    const fixture = ompDialogs.find((entry) => entry.name === "secret input")!;
    const request = AgentPermissionRequestPayloadSchema.parse(fixture.request);
    const onRespond = vi.fn();
    render(
      <QuestionFormCard
        permission={{ key: `agent:${request.id}`, agentId: "agent", request }}
        onRespond={onRespond}
        isResponding={false}
      />,
    );
    const input = screen.getByLabelText("API token") as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: "  hunter2  " } });
    fireEvent.click(screen.getByTestId("question-form-primary-action"));
    expect(onRespond).toHaveBeenCalledExactlyOnceWith({
      ...fixture.response,
      updatedInput: { ...request.input, ...fixture.response.updatedInput },
    });
  });

  it("submits an empty allow for optional input without typing", () => {
    const fixture = ompDialogs.find((entry) => entry.name === "empty input")!;
    const request = AgentPermissionRequestPayloadSchema.parse(fixture.request);
    const onRespond = vi.fn();
    render(
      <QuestionFormCard
        permission={{ key: `agent:${request.id}`, agentId: "agent", request }}
        onRespond={onRespond}
        isResponding={false}
      />,
    );
    fireEvent.click(screen.getByTestId("question-form-primary-action"));
    expect(onRespond).toHaveBeenCalledExactlyOnceWith({
      ...fixture.response,
      updatedInput: { ...request.input, ...fixture.response.updatedInput },
    });
  });

  it("prefills the editor draft and cancels on dismiss instead of submitting empty", () => {
    const fixture = ompDialogs.find((entry) => entry.name === "editor")!;
    const request = AgentPermissionRequestPayloadSchema.parse(fixture.request);
    const onRespond = vi.fn();
    render(
      <QuestionFormCard
        permission={{ key: `agent:${request.id}`, agentId: "agent", request }}
        onRespond={onRespond}
        isResponding={false}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Edit the plan" }) as HTMLTextAreaElement;
    expect(input.value).toBe("Initial plan");
    fireEvent.click(screen.getByTestId("question-form-dismiss"));
    expect(onRespond).toHaveBeenCalledExactlyOnceWith({
      behavior: "deny",
      message: "Dismissed by user",
    });
  });
});
