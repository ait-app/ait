// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GitAction } from "./policy";
import { GitActionsSplitButton } from "./actions-split-button";

vi.mock("@/git/use-actions", () => ({
  useGitActionRunner: () => (action: GitAction) => action.handler(),
}));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => null }));
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenu: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children, testID }: { children: ReactNode; testID: string }) => (
    <button data-testid={testID}>{children}</button>
  ),
  DropdownMenuItem: ({
    children,
    testID,
    onSelect,
    disabled,
  }: {
    children: ReactNode;
    testID: string;
    onSelect: () => void;
    disabled: boolean;
  }) => (
    <button data-testid={testID} onClick={onSelect} disabled={disabled}>
      {children}
    </button>
  ),
  DropdownMenuSeparator: () => <hr />,
}));

beforeEach(() => vi.stubGlobal("React", React));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("workspace action dropdown", () => {
  it("keeps manual archive reachable when there is no recommended primary action", () => {
    const handler = vi.fn();
    const archive: GitAction = {
      id: "archive-workspace",
      label: "Archive workspace",
      pendingLabel: "Archiving...",
      successLabel: "Archived",
      disabled: false,
      status: "idle",
      startsGroup: true,
      handler,
    };
    render(
      <GitActionsSplitButton gitActions={{ primary: null, secondary: [archive], menu: [] }} />,
    );

    expect(screen.queryByTestId("changes-primary-cta")).toBeNull();
    expect(screen.getByTestId("changes-actions-menu-trigger")).toBeTruthy();
    fireEvent.click(screen.getByTestId("workspace-archive-action"));
    expect(handler).toHaveBeenCalledOnce();
  });
});
