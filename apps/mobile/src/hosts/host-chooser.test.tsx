// @vitest-environment jsdom
import React, { forwardRef, useEffect, useImperativeHandle } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import type { HostProfile } from "@/types/host-connection";
import { HostChooserModal, useHostChooser, type ChooseHostInput } from "./host-chooser";

function host(serverId: string): HostProfile {
  return {
    serverId,
    label: serverId,
    appearance: {} as HostProfile["appearance"],
    lifecycle: {},
    connections: [],
    preferredConnectionId: null,
    createdAt: "2026-11-14T00:00:00.000Z",
    updatedAt: "2026-11-14T00:00:00.000Z",
  };
}

let hosts: HostProfile[];
let handleKey: (event: KeyboardEvent) => boolean;

vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => hosts,
  getHostRuntimeStore: () => ({ getAvailableHosts: () => hosts }),
}));
vi.mock("@/hooks/use-is-local-daemon", () => ({ useLocalDaemonServerId: () => null }));
vi.mock("@/components/hosts/host-picker", () => ({ HostStatusDotSlot: () => null }));
vi.mock("@/lib/overlay-root", () => ({
  OverlayLayerProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useGlobalWebOverlayLayer: () => 0,
  useWebOverlayRegistration: ({ onKeyDown }: { onKeyDown: typeof handleKey }) => {
    handleKey = onKeyDown;
    return () => undefined;
  },
}));
vi.mock("@/components/ui/text-input", () => ({
  EditingTextInput: forwardRef<
    { replaceText: (text: string) => void; focus: () => void },
    { placeholder?: string; onChangeText?: (text: string) => void }
  >(function EditingTextInput({ placeholder, onChangeText }, ref) {
    useImperativeHandle(ref, () => ({ replaceText: () => undefined, focus: () => undefined }));
    return (
      <input placeholder={placeholder} onChange={(event) => onChangeText?.(event.target.value)} />
    );
  }),
}));

function Harness({ input }: { input: Omit<ChooseHostInput, "onChooseHost"> }) {
  const chooseHost = useHostChooser();
  useEffect(() => {
    chooseHost({ ...input, onChooseHost: () => undefined });
  }, [chooseHost, input]);
  return <HostChooserModal />;
}

beforeEach(() => {
  hosts = [host("host-a"), host("host-b"), host("host-c")];
  vi.stubGlobal("React", React);
});

afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("en");
});

it("titles the chooser in the current language when the caller gives no title", async () => {
  await i18n.changeLanguage("fr");

  await act(async () => {
    render(<Harness input={{}} />);
  });

  expect(screen.getByText(i18n.getFixedT("fr")("addProjectFlow.titles.host"))).toBeTruthy();
  expect(screen.queryByText("Choose host")).toBeNull();
});

it("translates the search placeholder and the no-match state", async () => {
  await i18n.changeLanguage("fr");
  const fr = i18n.getFixedT("fr");

  await act(async () => {
    render(<Harness input={{}} />);
  });
  const search = screen.getByPlaceholderText(fr("addProjectFlow.placeholders.hosts"));
  await act(async () => {
    fireEvent.change(search, { target: { value: "no-such-host" } });
  });

  expect(screen.getByText(fr("addProjectFlow.empty.noMatchingHosts"))).toBeTruthy();
  expect(screen.queryByPlaceholderText("Search hosts...")).toBeNull();
  expect(screen.queryByText("No matching hosts")).toBeNull();
});

it("keeps a title the caller passes", async () => {
  await act(async () => {
    render(<Harness input={{ title: "Import into which host?" }} />);
  });

  expect(screen.getByText("Import into which host?")).toBeTruthy();
});

it("keeps the keyboard target when another host disappears and skips unavailable hosts", () => {
  const onChooseHost = vi.fn();
  function Launcher() {
    const choose = useHostChooser();
    return (
      <>
        <button onClick={() => choose({ onChooseHost })}>Choose</button>
        <HostChooserModal />
      </>
    );
  }
  const view = render(<Launcher />);
  fireEvent.click(screen.getByText("Choose"));
  const press = (key: string) =>
    act(() => {
      handleKey(new KeyboardEvent("keydown", { key }));
    });
  press("ArrowDown");
  const [first, second, third] = hosts;
  hosts = [third];
  view.rerender(<Launcher />);
  expect(screen.getByTestId("host-chooser-row-host-b").getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(screen.getByTestId("host-chooser-row-host-b"));
  press("Enter");
  expect(onChooseHost).not.toHaveBeenCalled();
  press("ArrowUp");
  press("Enter");
  expect(onChooseHost).toHaveBeenCalledExactlyOnceWith(third.serverId);

  hosts = [first, second, third];
  view.rerender(<Launcher />);
  fireEvent.click(screen.getByText("Choose"));
  press("ArrowDown");
  hosts = [second, third];
  view.rerender(<Launcher />);
  press("Enter");
  expect(onChooseHost).toHaveBeenLastCalledWith(second.serverId);
});
