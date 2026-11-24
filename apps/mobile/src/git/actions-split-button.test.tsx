// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GitControl, GitControlAction, GitControlPrimary } from "./control";
import "@/i18n/i18next";
import { GitActionsSplitButton } from "./actions-split-button";

vi.mock("@/git/use-actions", () => ({
  useGitActionRunner: () => (action: GitControlAction) => {
    if (action.disabled || action.unavailableReason) return;
    action.handler();
  },
}));
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenu: ({ children, open }: { children: ReactNode; open?: boolean }) => (
    <div data-testid="git-menu" data-open={String(Boolean(open))}>
      {children}
    </div>
  ),
  DropdownMenuContent: ({
    children,
    pages,
    testID,
  }: {
    children: ReactNode;
    pages?: readonly { id: string; content: ReactNode }[];
    testID: string;
  }) => (
    <div data-testid={testID}>
      {children}
      {pages?.map((page) => (
        <div key={page.id} data-testid={`page-${page.id}`}>
          {page.content}
        </div>
      ))}
    </div>
  ),
  DropdownMenuTrigger: ({ children, testID }: { children: ReactNode; testID: string }) => (
    <button data-testid={testID}>{children}</button>
  ),
  DropdownMenuItem: ({
    children,
    testID,
    onSelect,
    disabled,
    description,
    destructive,
    selected,
  }: {
    children: ReactNode;
    testID: string;
    onSelect: () => void;
    disabled?: boolean;
    description?: string;
    destructive?: boolean;
    selected?: boolean;
  }) => (
    <button
      data-testid={testID}
      onClick={onSelect}
      disabled={disabled}
      data-destructive={String(Boolean(destructive))}
      data-selected={String(Boolean(selected))}
    >
      {children}
      {description ? <span>{description}</span> : null}
    </button>
  ),
  DropdownMenuLabel: ({ children }: { children: ReactNode }) => (
    <div data-testid="menu-label">{children}</div>
  ),
  DropdownMenuHint: ({ children, testID }: { children: ReactNode; testID?: string }) => (
    <div data-testid={testID}>{children}</div>
  ),
  DropdownMenuSubTrigger: ({
    children,
    testID,
    value,
  }: {
    children: ReactNode;
    testID?: string;
    value?: string;
  }) => (
    <div data-testid={testID}>
      {children} {value}
    </div>
  ),
  DropdownMenuSeparator: () => <hr />,
}));

beforeEach(() => vi.stubGlobal("React", React));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function action(
  id: GitControlAction["id"],
  label: string,
  overrides: Partial<GitControlAction> = {},
): GitControlAction {
  return {
    id,
    label,
    pendingLabel: `${label}...`,
    successLabel: `${label} done`,
    status: "idle",
    disabled: false,
    handler: vi.fn(),
    ...overrides,
  };
}

function actionPrimary(primaryAction: GitControlAction, label = primaryAction.label) {
  return {
    kind: "action",
    action: primaryAction,
    label,
    incoming: null,
    outgoing: null,
  } satisfies GitControlPrimary;
}

function control(overrides: Partial<GitControl> = {}): GitControl {
  return {
    primary: { kind: "state", state: "up-to-date", label: "Up to date", tone: "neutral" },
    sections: [],
    mergeMethod: null,
    summary: { branch: "feature", base: "main", facts: ["2 ahead", "Clean"] },
    ...overrides,
  };
}

describe("GitActionsSplitButton", () => {
  it("runs the action's handler when the main button holds an action", () => {
    const commit = action("commit", "Commit");
    render(<GitActionsSplitButton control={control({ primary: actionPrimary(commit) })} />);

    fireEvent.click(screen.getByTestId("changes-primary-cta"));

    expect(commit.handler).toHaveBeenCalledOnce();
  });

  it("opens the panel from a state on the main button without running anything", () => {
    const pull = action("pull", "Pull");
    render(
      <GitActionsSplitButton
        control={control({
          primary: { kind: "state", state: "diverged", label: "Diverged", tone: "warning" },
          sections: [{ id: "sync", label: "Sync", actions: [pull] }],
        })}
      />,
    );
    const button = screen.getByTestId("changes-primary-cta");
    expect(button.getAttribute("aria-disabled")).not.toBe("true");
    expect(screen.getByTestId("git-menu").dataset.open).toBe("false");

    fireEvent.click(button);

    expect(screen.getByTestId("git-menu").dataset.open).toBe("true");
    expect(pull.handler).not.toHaveBeenCalled();
  });

  it("never renders a separate merge button", () => {
    const mergePr = action("merge-pr", "Merge PR (squash)");
    render(
      <GitActionsSplitButton
        control={control({
          primary: actionPrimary(action("pr", "View PR")),
          sections: [{ id: "pull-request", label: "Pull request", actions: [mergePr] }],
        })}
      />,
    );

    expect(screen.queryByTestId("changes-merge-pr-cta")).toBeNull();
  });

  it("renders section labels in order and an unavailable action disabled with its reason", () => {
    render(
      <GitActionsSplitButton
        control={control({
          sections: [
            { id: "changes", label: "Changes", actions: [action("commit", "Commit")] },
            {
              id: "sync",
              label: "Sync",
              actions: [action("pull", "Pull", { unavailableReason: "Already up to date" })],
            },
            { id: "local", label: "Local", actions: [action("merge-branch", "Merge into main")] },
          ],
        })}
      />,
    );

    expect(screen.getAllByTestId("menu-label").map((label) => label.textContent)).toEqual([
      "Changes",
      "Sync",
      "Local",
    ]);
    const pull = screen.getByTestId("changes-menu-pull");
    expect((pull as HTMLButtonElement).disabled).toBe(true);
    expect(pull.textContent).toContain("Already up to date");
  });

  it("marks reset workspace as destructive", () => {
    const reset = action("reset-workspace", "Reset workspace", {
      destructive: true,
      description: "Discard everything and return to the latest main",
    });
    render(
      <GitActionsSplitButton
        control={control({ sections: [{ id: "danger", label: null, actions: [reset] }] })}
      />,
    );

    expect(screen.getByTestId("changes-menu-reset-workspace").dataset.destructive).toBe("true");
  });

  it("passes the method chosen on the merge-method page to onSelect", () => {
    const onSelect = vi.fn();
    render(
      <GitActionsSplitButton
        control={control({
          primary: actionPrimary(action("pr", "View PR")),
          sections: [
            {
              id: "pull-request",
              label: "Pull request",
              actions: [action("merge-pr", "Merge PR (squash)")],
            },
          ],
          mergeMethod: {
            label: "Merge method",
            selected: "squash",
            options: [
              { method: "squash", label: "Squash" },
              { method: "rebase", label: "Rebase" },
            ],
            onSelect,
          },
        })}
      />,
    );
    const page = screen.getByTestId("page-git-merge-method");
    expect(within(page).getByTestId("changes-merge-method-squash").dataset.selected).toBe("true");

    fireEvent.click(within(page).getByTestId("changes-merge-method-rebase"));

    expect(onSelect).toHaveBeenCalledWith("rebase");
  });

  it("renders the same sections behind a menu trigger when menu-only", () => {
    render(
      <GitActionsSplitButton
        menuOnly
        control={control({
          primary: actionPrimary(action("commit", "Commit")),
          sections: [
            { id: "changes", label: "Changes", actions: [action("commit", "Commit")] },
            { id: "sync", label: "Sync", actions: [action("push", "Push")] },
          ],
        })}
      />,
    );

    expect(screen.getByTestId("changes-actions-menu-trigger")).toBeTruthy();
    expect(screen.queryByTestId("changes-primary-cta")).toBeNull();
    expect(screen.getAllByTestId("menu-label").map((label) => label.textContent)).toEqual([
      "Changes",
      "Sync",
    ]);
    expect(screen.getByTestId("changes-menu-push")).toBeTruthy();
  });

  it("shows the branch summary and its facts at the top of the panel", () => {
    render(
      <GitActionsSplitButton
        control={control({
          sections: [{ id: "sync", label: "Sync", actions: [action("push", "Push")] }],
        })}
      />,
    );
    const menu = screen.getByTestId("changes-primary-cta-menu");
    const branch = within(menu).getByTestId("changes-git-summary-branch");
    const facts = within(menu).getByTestId("changes-git-summary-facts");

    expect(branch.textContent).toBe("feature → main");
    expect(facts.textContent).toBe("2 ahead · Clean");
    const firstLabel = within(menu).getAllByTestId("menu-label")[0];
    for (const row of [branch, facts]) {
      expect(
        row.compareDocumentPosition(firstLabel) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    }
  });
});
