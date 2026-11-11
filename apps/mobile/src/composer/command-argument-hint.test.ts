// @vitest-environment jsdom

import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentSlashCommand } from "@/hooks/use-agent-commands-query";
import { applyNativeCommandCompletion } from "@/utils/native-command-completion";
import {
  commandArgumentHint,
  getImeComposingSnapshot,
  isCommandArgumentArea,
  subscribeToImeComposition,
} from "./command-argument-hint";
import { InlineCommandHint } from "./inline-command-hint";

const commandCatalog = vi.hoisted(() => ({ current: [] as AgentSlashCommand[] }));

vi.mock("@/hooks/use-agent-commands-query", () => ({
  useAgentCommandsQuery: () => ({ commands: commandCatalog.current }),
}));

const commands: AgentSlashCommand[] = [
  {
    name: "guided-goal",
    description: "Clarify a goal",
    argumentHint: "[rough objective]",
    source: "builtin",
  },
  {
    name: "deploy",
    description: "Deploy",
    argumentHint: "[env]",
    source: "plugin",
    aliases: ["deploy:"],
  },
  { name: "bare", description: "No metadata", argumentHint: "", source: "skill" },
  {
    name: "wrapped",
    description: "Metadata with its own line break",
    argumentHint: "[first\nsecond]",
    source: "plugin",
  },
];

const subscriptions: Array<() => void> = [];
const nodes: Node[] = [];

function subscribe(listener: () => void): void {
  subscriptions.push(subscribeToImeComposition(listener));
}

function editor(): HTMLElement {
  const element = document.createElement("textarea");
  document.body.append(element);
  nodes.push(element);
  return element;
}

function compose(target: EventTarget, type: "compositionstart" | "compositionend"): void {
  target.dispatchEvent(new CompositionEvent(type, { bubbles: true }));
}

afterEach(() => {
  for (const stop of subscriptions.splice(0)) stop();
  for (const node of nodes.splice(0)) node.parentNode?.removeChild(node);
});

describe("commandArgumentHint", () => {
  it("shows declared metadata only for an empty argument area", () => {
    expect(commandArgumentHint("/guided-goal ", commands)).toEqual({
      prefix: "/guided-goal ",
      hint: "[rough objective]",
    });
    expect(commandArgumentHint("/deploy  ", commands)).toEqual({
      prefix: "/deploy  ",
      hint: "[env]",
    });
  });

  it("matches native aliases without inventing hints", () => {
    expect(commandArgumentHint("/deploy: ", commands)?.hint).toBe("[env]");
    expect(commandArgumentHint("/bare ", commands)).toBeNull();
    expect(commandArgumentHint("/unknown ", commands)).toBeNull();
  });

  it("never produces a hint once the user types arguments or edits multiline text", () => {
    expect(commandArgumentHint("/guided-goal build a", commands)).toBeNull();
    expect(commandArgumentHint("/guided-goal\n", commands)).toBeNull();
    expect(commandArgumentHint("plain text", commands)).toBeNull();
    expect(commandArgumentHint("", commands)).toBeNull();
  });

  it("rejects metadata that cannot be painted on one line", () => {
    expect(commandArgumentHint("/wrapped ", commands)).toBeNull();
  });
});

describe("isCommandArgumentArea", () => {
  it("accepts only separators native `split` discards", () => {
    expect(isCommandArgumentArea("/deploy ")).toBe(true);
    expect(isCommandArgumentArea("/deploy\t")).toBe(true);
    expect(isCommandArgumentArea("/deploy:")).toBe(true);
    expect(isCommandArgumentArea("/deploy:  ")).toBe(true);
    expect(isCommandArgumentArea("/deploy::")).toBe(true);
  });

  it("rejects text native `split` reports as a real argument", () => {
    // Native strips leading colons only, so a colon after whitespace is already the argument.
    expect(isCommandArgumentArea("/deploy :")).toBe(false);
    expect(isCommandArgumentArea("/deploy\t:")).toBe(false);
    expect(isCommandArgumentArea("/deploy:\t:")).toBe(false);
    expect(isCommandArgumentArea("/deploy: :")).toBe(false);
    expect(isCommandArgumentArea("/deploy env")).toBe(false);
    expect(isCommandArgumentArea("/deploy")).toBe(false);
    expect(isCommandArgumentArea("/deploy\n")).toBe(false);
    // The overlay is left-aligned, so a leading separator would misplace it even though the
    // native split trims it away.
    expect(isCommandArgumentArea(" /deploy ")).toBe(false);
    expect(isCommandArgumentArea("deploy ")).toBe(false);
    expect(isCommandArgumentArea("/ ")).toBe(false);
    expect(isCommandArgumentArea("//deploy ")).toBe(false);
  });
});

describe("native completion feeding the hint", () => {
  it("turns a completed name into an empty argument area with its declared hint", () => {
    const edited = applyNativeCommandCompletion("/dep", {
      id: "deploy",
      label: "/deploy",
      insertText: "/deploy ",
      replaceRange: { start: 0, end: 4 },
      triggerCompletion: false,
    });
    expect(edited).toEqual({ text: "/deploy ", cursor: 8 });
    expect(commandArgumentHint(edited!.text, commands)).toEqual({
      prefix: "/deploy ",
      hint: "[env]",
    });
  });

  it("keeps the hint hidden when the native edit carries its own argument", () => {
    const edited = applyNativeCommandCompletion("/dep", {
      id: "deploy",
      label: "/deploy prod",
      insertText: "/deploy prod",
      replaceRange: { start: 0, end: 4 },
      triggerCompletion: false,
    });
    expect(edited).toEqual({ text: "/deploy prod", cursor: 12 });
    expect(commandArgumentHint(edited!.text, commands)).toBeNull();
  });
});

describe("IME composition", () => {
  it("stays quiet outside a composition and tracks it until commit", () => {
    const input = editor();
    const listener = vi.fn();
    subscribe(listener);
    expect(getImeComposingSnapshot()).toBe(false);

    compose(input, "compositionstart");
    expect(getImeComposingSnapshot()).toBe(true);
    expect(listener).toHaveBeenCalledTimes(1);

    // Composition updates keep the candidate active instead of flickering the hint back on.
    compose(input, "compositionstart");
    expect(getImeComposingSnapshot()).toBe(true);

    compose(input, "compositionend");
    expect(getImeComposingSnapshot()).toBe(false);
    expect(listener).toHaveBeenCalledTimes(3);
  });

  it("keeps the hint suppressed while any editor still composes", () => {
    const first = editor();
    const second = editor();
    subscribe(() => {});
    compose(first, "compositionstart");
    compose(second, "compositionstart");
    compose(first, "compositionend");
    expect(getImeComposingSnapshot()).toBe(true);
    compose(second, "compositionend");
    expect(getImeComposingSnapshot()).toBe(false);
  });

  it("never strands a candidate whose editor left the document", () => {
    const input = editor();
    subscribe(() => {});
    compose(input, "compositionstart");
    expect(getImeComposingSnapshot()).toBe(true);
    input.remove();
    expect(getImeComposingSnapshot()).toBe(false);
  });

  it("detaches DOM observation with the last subscriber", () => {
    const input = editor();
    const listener = vi.fn();
    subscribe(listener);
    subscribe(() => {});
    subscriptions.pop()?.();
    compose(input, "compositionstart");
    expect(listener).toHaveBeenCalledTimes(1);
    subscriptions.pop()?.();
    compose(input, "compositionend");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(getImeComposingSnapshot()).toBe(false);

    const lateListener = vi.fn();
    subscribe(lateListener);
    compose(input, "compositionstart");
    expect(lateListener).toHaveBeenCalledTimes(1);
  });
});

describe("InlineCommandHint", () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  function draft(text: string) {
    let current = text;
    const listeners = new Set<() => void>();
    return {
      source: {
        getSnapshot: () => current,
        subscribe: (listener: () => void) => {
          listeners.add(listener);
          return () => listeners.delete(listener);
        },
      },
      type(next: string) {
        current = next;
        for (const listener of listeners) listener();
      },
    };
  }

  function renderHint(text: ReturnType<typeof draft>) {
    commandCatalog.current = commands;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    act(() => {
      root?.render(
        createElement(InlineCommandHint, {
          text: text.source,
          serverId: "server",
          agentId: "agent",
        }),
      );
    });
    return container;
  }

  function hintText(host: HTMLElement): string | null {
    const text =
      host.querySelector('[data-testid="composer-command-argument-hint"]')?.textContent ?? null;
    // The decorative prefix is painted with no-break spaces so it cannot wrap; compare words.
    return text?.replaceAll("\u00a0", " ") ?? null;
  }

  afterEach(() => {
    if (root) act(() => root?.unmount());
    root = null;
    container = null;
  });

  it("paints the declared hint next to the typed command without editing it", () => {
    const text = draft("/deploy ");
    const host = renderHint(text);
    expect(hintText(host)).toBe("/deploy [env]");
    expect(text.source.getSnapshot()).toBe("/deploy ");
  });

  it("yields to the composition candidate and returns once it commits", () => {
    const text = draft("/deploy ");
    const host = renderHint(text);
    const input = editor();
    expect(hintText(host)).toBe("/deploy [env]");

    act(() => compose(input, "compositionstart"));
    expect(hintText(host)).toBeNull();

    // Composition updates must not bring the overlay back over the live candidate.
    act(() => {
      text.type("/deploy a");
      compose(input, "compositionstart");
    });
    expect(hintText(host)).toBeNull();

    act(() => compose(input, "compositionend"));
    expect(hintText(host)).toBeNull();
    act(() => text.type("/deploy "));
    expect(hintText(host)).toBe("/deploy [env]");
  });

  it("never shows the hint while the draft is not an empty argument area", () => {
    const text = draft("/dep");
    const host = renderHint(text);
    expect(hintText(host)).toBeNull();
    act(() => text.type("/deploy prod"));
    expect(hintText(host)).toBeNull();
    act(() => text.type("/deploy "));
    expect(hintText(host)).toBe("/deploy [env]");
    act(() => text.type("/deploy\n"));
    expect(hintText(host)).toBeNull();
  });
});
