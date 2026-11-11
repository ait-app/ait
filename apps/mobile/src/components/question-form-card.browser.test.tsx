import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AgentPermissionRequestPayloadSchema } from "@ait/protocol/messages";
import type { AgentPermissionResponse } from "@ait/protocol/agent-types";
import { QuestionFormCard } from "./question-form-card";
import ompDialogs from "../../../../crates/provider/tests/fixtures/omp_dialogs.json";

const mounted: { root: Root; container: HTMLDivElement }[] = [];

function mountRequest<T extends { request: unknown }>(
  fixture: T,
  onRespond: (response: AgentPermissionResponse) => void,
) {
  const request = AgentPermissionRequestPayloadSchema.parse(fixture.request);
  const container = document.createElement("div");
  container.style.width = "390px";
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() =>
    root.render(
      <QuestionFormCard
        permission={{ key: `agent:${request.id}`, agentId: "agent", request }}
        onRespond={onRespond}
        isResponding={false}
      />,
    ),
  );
  mounted.push({ root, container });
  return { container, fixture, request };
}

function mountForm(name: string, onRespond: (response: AgentPermissionResponse) => void) {
  const fixture = ompDialogs.find((entry) => entry.name === name);
  if (!fixture) throw new Error(`missing fixture ${name}`);
  return mountRequest(fixture, onRespond);
}

function setInputValue(input: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto =
    input instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  if (!setter) throw new Error("value setter unavailable");
  act(() => {
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

afterEach(() => {
  for (const entry of mounted.splice(0)) {
    act(() => entry.root.unmount());
    entry.container.remove();
  }
});

describe("OMP structured form browser smoke", () => {
  it("masks secrets, keeps empty/whitespace values and cancels without submitting", () => {
    const responses: AgentPermissionResponse[] = [];
    const onRespond = vi.fn((response: AgentPermissionResponse) => responses.push(response));

    const secret = mountForm("secret input", onRespond);
    const secretInput = secret.container.querySelector<HTMLInputElement>('input[type="password"]');
    expect(secretInput).toBeTruthy();
    setInputValue(secretInput!, "  hunter2  ");
    act(() => {
      secret.container
        .querySelector<HTMLElement>('[data-testid="question-form-primary-action"]')!
        .click();
    });
    expect(responses.at(-1)).toEqual({
      behavior: "allow",
      updatedInput: {
        ...secret.fixture.request.input,
        answers: { "API token": "  hunter2  " },
      },
    });
    act(() => mounted.pop()!.root.unmount());

    const empty = mountForm("empty input", onRespond);
    act(() => {
      empty.container
        .querySelector<HTMLElement>('[data-testid="question-form-primary-action"]')!
        .click();
    });
    expect(responses.at(-1)).toEqual({
      behavior: "allow",
      updatedInput: {
        ...empty.fixture.request.input,
        answers: { "Optional note": "" },
      },
    });
    act(() => mounted.pop()!.root.unmount());

    const editor = mountForm("editor", onRespond);
    const textarea = editor.container.querySelector<HTMLTextAreaElement>("textarea");
    expect(textarea?.value).toBe("Initial plan");
    setInputValue(textarea!, "Updated plan\nwith a second line");
    act(() => {
      editor.container
        .querySelector<HTMLElement>('[data-testid="question-form-primary-action"]')!
        .click();
    });
    expect(responses.at(-1)).toEqual({
      behavior: "allow",
      updatedInput: {
        ...editor.fixture.request.input,
        answers: { "Edit the plan": "Updated plan\nwith a second line" },
      },
    });
    act(() => mounted.pop()!.root.unmount());

    const cancel = mountForm("text input", onRespond);
    act(() => {
      cancel.container.querySelector<HTMLElement>('[data-testid="question-form-dismiss"]')!.click();
    });
    expect(responses.at(-1)).toEqual({ behavior: "deny", message: "Dismissed by user" });
  });

  it("keeps a secret multiline request single-line and blocks an empty required submit", () => {
    const responses: AgentPermissionResponse[] = [];
    const onRespond = vi.fn((response: AgentPermissionResponse) => responses.push(response));
    const secretMultiline = {
      request: {
        id: "required-passphrase",
        provider: "omp",
        name: "extension_ui",
        kind: "question",
        input: {
          questions: [
            {
              header: "Passphrase",
              question: "Passphrase",
              answerKey: "value",
              options: [],
              multiSelect: false,
              allowOther: true,
              allowEmpty: false,
              secret: true,
              multiline: true,
            },
          ],
        },
      },
    };
    const required = mountRequest(secretMultiline, onRespond);
    expect(required.container.querySelector("textarea")).toBeNull();
    const input = required.container.querySelector<HTMLInputElement>('input[type="password"]');
    expect(input).toBeTruthy();
    const primary = required.container.querySelector<HTMLElement>(
      '[data-testid="question-form-primary-action"]',
    )!;
    expect(primary.getAttribute("aria-disabled")).toBe("true");
    act(() => primary.click());
    expect(responses).toHaveLength(0);

    setInputValue(input!, "  correct horse  ");
    act(() => primary.click());
    expect(responses.at(-1)).toEqual({
      behavior: "allow",
      updatedInput: {
        ...secretMultiline.request.input,
        answers: { Passphrase: "  correct horse  " },
      },
    });
  });

  it("clears custom text when selecting an option without leaving the question", () => {
    const onRespond = vi.fn();
    const { container, request } = mountRequest(
      {
        request: {
          id: "choice",
          provider: "omp",
          name: "extension_ui",
          kind: "question",
          input: {
            questions: [
              {
                header: "Choice",
                question: "Choose",
                options: [{ label: "First" }],
                multiSelect: false,
                allowOther: true,
                initialValue: "prefilled",
              },
            ],
          },
        },
      },
      onRespond,
    );
    setInputValue(container.querySelector<HTMLInputElement>("input")!, "custom");
    act(() => container.querySelector<HTMLElement>('[role="radio"]')!.click());
    expect(container.querySelector<HTMLInputElement>("input")?.value).toBe("");
    act(() =>
      container.querySelector<HTMLElement>('[data-testid="question-form-primary-action"]')!.click(),
    );
    expect(onRespond).toHaveBeenCalledWith({
      behavior: "allow",
      updatedInput: { ...request.input, answers: { Choice: "First" } },
    });
  });
});
