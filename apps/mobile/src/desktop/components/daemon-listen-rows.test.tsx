/** @vitest-environment jsdom */
import React, { act } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DaemonListenRows } from "./daemon-listen-rows";

const save = vi.fn();
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: unknown) => `${key}${values ? JSON.stringify(values) : ""}`,
  }),
}));
vi.mock("@/styles/settings", () => ({ settingsStyles: {} }));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: {
    create: (factory: (theme: unknown) => unknown) =>
      factory({ spacing: {}, colors: {}, borderRadius: {}, borderWidth: {}, fontSize: {} }),
  },
}));
vi.mock("@/components/ui/text-input", () => ({
  EditingTextInput: ({
    initialValue,
    onChangeText,
    onBlur,
    onSubmitEditing,
    testID,
  }: {
    initialValue: string;
    onChangeText: (value: string) => void;
    onBlur: () => void;
    onSubmitEditing: () => void;
    testID: string;
  }) => (
    <input
      data-testid={testID}
      defaultValue={initialValue}
      onChange={(event) => onChangeText(event.target.value)}
      onBlur={onBlur}
      onKeyDown={(event) => {
        if (event.key === "Enter") onSubmitEditing();
      }}
    />
  ),
}));

beforeEach(() => {
  save.mockReset().mockResolvedValue(undefined);
});
afterEach(cleanup);

function show(override: string | null = null) {
  render(<DaemonListenRows listen="127.0.0.1:0" override={override} onChangeListen={save} />);
  return {
    host: screen.getByTestId("server-listen-host") as HTMLInputElement,
    port: screen.getByTestId("server-listen-port") as HTMLInputElement,
  };
}

it("uses settings rows without a section or button and commits on blur or Enter", async () => {
  const { host, port } = show();
  expect(host.value).toBe("127.0.0.1");
  expect(port.value).toBe("0");
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByText("desktop.daemon.listen.savedHint")).toBeTruthy();
  expect(screen.queryByText("desktop.daemon.listen.title")).toBeNull();
  fireEvent.change(host, { target: { value: "0.0.0.0" } });
  expect(save).not.toHaveBeenCalled();
  fireEvent.blur(host);
  await waitFor(() => expect(save).toHaveBeenCalledWith("0.0.0.0:0"));
  fireEvent.change(port, { target: { value: "7316" } });
  fireEvent.keyDown(port, { key: "Enter" });
  fireEvent.blur(port);
  await waitFor(() => expect(save).toHaveBeenLastCalledWith("0.0.0.0:7316"));
  expect(save).toHaveBeenCalledTimes(2);
});

it("keeps invalid input out of the saved configuration", async () => {
  const { port } = show();
  fireEvent.change(port, { target: { value: "65536" } });
  fireEvent.blur(port);
  expect(screen.getByText("desktop.daemon.listen.invalid")).toBeTruthy();
  expect(save).not.toHaveBeenCalled();
  fireEvent.change(port, { target: { value: "7317" } });
  fireEvent.blur(port);
  await waitFor(() => expect(save).toHaveBeenCalledWith("127.0.0.1:7317"));
});

it("preserves a failed draft and allows Enter to retry it", async () => {
  save.mockRejectedValueOnce(new Error("disk unavailable"));
  const { port } = show();
  fireEvent.change(port, { target: { value: "7317" } });
  fireEvent.blur(port);
  expect(await screen.findByText("desktop.settings.saveFailed")).toBeTruthy();
  expect(port.value).toBe("7317");
  fireEvent.keyDown(port, { key: "Enter" });
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(screen.queryByText("desktop.settings.saveFailed")).toBeNull());
});

it("queues a second field edit while the first commit is still saving", async () => {
  let finishFirst!: () => void;
  save.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finishFirst = resolve;
      }),
  );
  const { host, port } = show();
  fireEvent.change(host, { target: { value: "0.0.0.0" } });
  fireEvent.blur(host);
  await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
  fireEvent.change(port, { target: { value: "7316" } });
  fireEvent.blur(port);
  expect(save).toHaveBeenCalledTimes(1);
  await act(async () => finishFirst());
  await waitFor(() => expect(save).toHaveBeenLastCalledWith("0.0.0.0:7316"));
  expect(port.value).toBe("7316");
});

it("shows an environment override only as a row hint", () => {
  show("0.0.0.0:8080");
  expect(screen.getByText(/desktop.daemon.listen.override.*0.0.0.0:8080/)).toBeTruthy();
  expect(screen.queryByText("desktop.daemon.listen.savedHint")).toBeNull();
});
