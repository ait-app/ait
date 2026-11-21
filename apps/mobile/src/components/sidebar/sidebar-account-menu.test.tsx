// @vitest-environment jsdom
import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountInfo } from "@ait/client/internal/account-session";
import { i18n } from "@/i18n/i18next";
import { SidebarAccountMenu, useIsAccountSignedIn } from "./sidebar-account-menu";

const HOUR = 60 * 60 * 1000;
const DAY = 24 * HOUR;
const NOW = Date.parse("2026-11-22T00:00:00Z");

interface Account {
  status: "logged_out" | "connecting" | "online" | "error";
  center: string;
  name: string;
  accountId?: string;
  accountExpiresAt?: string | null;
}

const mocks = vi.hoisted(() => ({
  load: vi.fn<() => Promise<AccountInfo>>(),
  command: vi.fn<(command: string) => Promise<unknown>>(),
  toastError: vi.fn<(message: string) => void>(),
  account: {} as Account,
  visible: true,
}));

vi.mock("@/runtime/account-state", () => ({
  useAccountState: <T,>(selector: (state: Account) => T) => selector(mocks.account),
  loadAccountInfo: mocks.load,
  accountCommand: mocks.command,
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: mocks.toastError }) }));
vi.mock("@/hooks/use-app-visible", () => ({ useAppVisible: () => mocks.visible }));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => null }));
vi.mock("./sidebar-help-menu", () => ({
  SidebarHelpMenuItems: () => <div data-testid="sidebar-help-items" />,
}));
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenu: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({
    children,
    testID,
    accessibilityLabel,
  }: {
    children: ReactNode;
    testID: string;
    accessibilityLabel: string;
  }) => (
    <button data-testid={testID} aria-label={accessibilityLabel}>
      {children}
    </button>
  ),
  DropdownMenuItem: ({
    children,
    testID,
    onSelect,
  }: {
    children: ReactNode;
    testID: string;
    onSelect: () => void;
  }) => (
    <button data-testid={testID} onClick={onSelect}>
      {children}
    </button>
  ),
  DropdownMenuSeparator: () => <hr />,
}));

function isoIn(ms: number): string {
  return new Date(NOW + ms).toISOString();
}

function renderMenu(handlers?: { onOpenSettings?: () => void; onImportSession?: () => void }) {
  return render(
    <SidebarAccountMenu
      onOpenSettings={handlers?.onOpenSettings ?? vi.fn()}
      onImportSession={handlers?.onImportSession ?? vi.fn()}
    />,
  );
}

beforeEach(async () => {
  vi.useFakeTimers({ now: NOW, toFake: ["Date", "setTimeout", "clearTimeout"] });
  mocks.load.mockReset();
  mocks.load.mockRejectedValue(new Error("offline"));
  mocks.command.mockReset();
  mocks.command.mockResolvedValue(undefined);
  mocks.toastError.mockReset();
  mocks.visible = true;
  mocks.account = {
    status: "online",
    center: "https://example.test/api",
    name: "Neco",
    accountId: "account-1",
    accountExpiresAt: isoIn(3 * DAY),
  };
  await i18n.changeLanguage("en");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("SidebarAccountMenu", () => {
  it("shows who is signed in and how long the account has left", () => {
    const view = renderMenu();

    expect(view.getByTestId("sidebar-account-name").textContent).toBe("Neco");
    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("3 days left");
    expect(view.getByTestId("sidebar-account").getAttribute("aria-label")).toBe(
      "Neco, 3 days left",
    );
  });

  it.each([
    [95 * DAY, "Months left"],
    [40 * DAY, "Months left"],
    [20 * DAY, "Weeks left"],
    [8 * DAY, "Weeks left"],
    [5 * DAY, "5 days left"],
    [36 * HOUR, "36 hours left"],
    [30 * 60 * 1000, "1 hour left"],
    [-HOUR, "Expired"],
  ])("labels an expiry %i ms away as %s", (offset, label) => {
    mocks.account.accountExpiresAt = isoIn(offset);
    const view = renderMenu();

    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe(label);
  });

  it("says an account without an expiry never expires", () => {
    mocks.account.accountExpiresAt = null;
    const view = renderMenu();

    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("Never expires");
  });

  it("shows only the name while the expiry is unknown", () => {
    mocks.account.accountExpiresAt = undefined;
    const view = renderMenu();

    expect(view.getByTestId("sidebar-account-name").textContent).toBe("Neco");
    expect(view.queryByTestId("sidebar-account-expiry")).toBeNull();
  });

  it("prefers the expiry the service reports over the one saved at sign-in", async () => {
    mocks.load.mockResolvedValue({ loginName: "neco@example.test", expiresAt: isoIn(95 * DAY) });
    const view = renderMenu();
    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("3 days left");

    await act(async () => {
      await Promise.resolve();
    });

    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("Months left");
  });

  it("does not ask the service while signed in but not connected", () => {
    mocks.account.status = "connecting";
    renderMenu();

    expect(mocks.load).not.toHaveBeenCalled();
  });

  it("counts down as the expiry approaches", () => {
    // One jump of the fake clock runs a single tick; the second act lets the re-armed one land.
    const advance = (ms: number) => {
      act(() => {
        vi.advanceTimersByTime(ms);
      });
      act(() => {
        vi.advanceTimersByTime(0);
      });
    };
    mocks.account.accountExpiresAt = isoIn(2 * DAY + HOUR);
    const view = renderMenu();
    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("2 days left");

    advance(HOUR + 1);
    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("47 hours left");

    advance(46 * HOUR);
    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("1 hour left");

    advance(2 * HOUR);
    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("Expired");
  });

  it("uses each language's plural forms", async () => {
    await i18n.changeLanguage("ru");
    mocks.account.accountExpiresAt = isoIn(5 * DAY);
    const view = renderMenu();

    expect(view.getByTestId("sidebar-account-expiry").textContent).toBe("Осталось 5 дней");
  });

  it("keeps Settings, Import session and Help reachable from its menu", () => {
    const onOpenSettings = vi.fn();
    const onImportSession = vi.fn();
    const view = renderMenu({ onOpenSettings, onImportSession });

    fireEvent.click(view.getByTestId("sidebar-account-settings"));
    fireEvent.click(view.getByTestId("sidebar-account-import-session"));

    expect(onOpenSettings).toHaveBeenCalledOnce();
    expect(onImportSession).toHaveBeenCalledOnce();
    expect(view.getByTestId("sidebar-help-items")).toBeTruthy();
  });

  it("ends with Sign out, which signs the account out", async () => {
    const view = renderMenu();

    const items = view.getAllByRole("button").map((item) => item.getAttribute("data-testid"));
    expect(items.at(-1)).toBe("sidebar-account-sign-out");

    fireEvent.click(view.getByTestId("sidebar-account-sign-out"));
    await act(async () => {
      await Promise.resolve();
    });

    expect(mocks.command).toHaveBeenCalledExactlyOnceWith("account_logout");
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("reports a failed sign-out", async () => {
    mocks.command.mockRejectedValue(new Error("The service is unreachable."));
    const view = renderMenu();

    fireEvent.click(view.getByTestId("sidebar-account-sign-out"));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(mocks.toastError).toHaveBeenCalledExactlyOnceWith("The service is unreachable.");
  });

  it("ignores a second Sign out while the first is still running", () => {
    mocks.command.mockReturnValue(new Promise(() => {}));
    const view = renderMenu();

    fireEvent.click(view.getByTestId("sidebar-account-sign-out"));
    fireEvent.click(view.getByTestId("sidebar-account-sign-out"));

    expect(mocks.command).toHaveBeenCalledOnce();
  });
});

describe("useIsAccountSignedIn", () => {
  function Probe() {
    return <span data-testid="signed-in">{String(useIsAccountSignedIn())}</span>;
  }

  it.each([
    ["logged_out", "false"],
    ["connecting", "true"],
    ["online", "true"],
    ["error", "true"],
  ] as const)("treats %s as signed in: %s", (status, expected) => {
    mocks.account.status = status;
    const view = render(<Probe />);

    expect(view.getByTestId("signed-in").textContent).toBe(expected);
  });
});
