// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PendingPermission } from "@/types/shared";
import { QuestionFormCard } from "./question-form-card";

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/components/ui/loading-spinner", () => ({
  LoadingSpinner: () => <span role="progressbar" />,
}));
// Like the real input, this one keeps its own text: `initialValue` is read once, on mount.
vi.mock("@/components/ui/text-input", () => ({
  EditingTextInput: ({
    initialValue,
    onChangeText,
    accessibilityLabel,
  }: {
    initialValue: string;
    onChangeText: (text: string) => void;
    accessibilityLabel: string;
  }) => (
    <input
      aria-label={accessibilityLabel}
      defaultValue={initialValue}
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

function otherInput(): HTMLInputElement {
  return screen.getByRole("textbox", { name: "Choose a runtime" }) as HTMLInputElement;
}

describe("question form free-text answer", () => {
  beforeEach(() => vi.stubGlobal("React", React));
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("clears the typed answer from the field when an option replaces it", () => {
    render(<QuestionFormCard permission={permission} onRespond={vi.fn()} isResponding={false} />);

    fireEvent.change(otherInput(), { target: { value: "Zig" } });
    expect(otherInput().value).toBe("Zig");

    fireEvent.click(screen.getByRole("radio", { name: "Rust" }));

    expect(otherInput().value).toBe("");
  });

  it("keeps the field as typed while no option replaces it", () => {
    render(<QuestionFormCard permission={permission} onRespond={vi.fn()} isResponding={false} />);

    const input = otherInput();
    fireEvent.change(input, { target: { value: "Zig" } });
    fireEvent.change(input, { target: { value: "Zig 0.14" } });

    expect(otherInput()).toBe(input);
    expect(otherInput().value).toBe("Zig 0.14");
  });
});
