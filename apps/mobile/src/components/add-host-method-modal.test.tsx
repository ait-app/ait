/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { AddHostMethodModal } from "./add-host-method-modal";

vi.mock("@/desktop/host", () => ({ isElectronRuntime: () => true }));
vi.mock("@/runtime/account-state", () => ({ supportsAccountRelay: () => true }));
vi.mock("./account-host-panel", () => ({
  AccountHostPanel: () => <input aria-label="Account login" />,
}));
vi.mock("./adaptive-modal-sheet", () => ({
  AdaptiveModalSheet: ({
    visible,
    header,
    children,
  }: {
    visible: boolean;
    header: { title: string; back?: { onPress: () => void } };
    children: React.ReactNode;
  }) =>
    visible ? (
      <div>
        <h1>{header.title}</h1>
        {header.back ? <button onClick={header.back.onPress}>Back</button> : null}
        {children}
      </div>
    ) : null,
}));
afterEach(() => cleanup());

describe("online service connection method", () => {
  it("keeps login in a second level and resets the sheet when it closes", async () => {
    await i18n.changeLanguage("zh-CN");
    const props = {
      visible: true,
      onClose: vi.fn(),
      onDirectConnection: vi.fn(),
      onRemoteSsh: vi.fn(),
    };
    const view = render(<AddHostMethodModal {...props} />);
    expect(view.getByTestId("add-host-method-online-service").textContent).toContain("在线服务");
    expect(view.getByTestId("add-host-method-direct")).toBeTruthy();
    expect(view.getByTestId("add-host-method-remote-ssh")).toBeTruthy();
    expect(view.getAllByRole("button").map((button) => button.getAttribute("data-testid"))).toEqual(
      ["add-host-method-direct", "add-host-method-online-service", "add-host-method-remote-ssh"],
    );
    expect(view.queryByLabelText("Account login")).toBeNull();
    fireEvent.click(view.getByTestId("add-host-method-online-service"));
    expect(view.getByLabelText("Account login")).toBeTruthy();
    expect(view.queryByTestId("add-host-method-direct")).toBeNull();
    fireEvent.click(view.getByText("Back"));
    expect(view.getByTestId("add-host-method-online-service")).toBeTruthy();
    fireEvent.click(view.getByTestId("add-host-method-direct"));
    expect(props.onDirectConnection).toHaveBeenCalledOnce();
    fireEvent.click(view.getByTestId("add-host-method-online-service"));
    view.rerender(<AddHostMethodModal {...props} visible={false} />);
    view.rerender(<AddHostMethodModal {...props} />);
    expect(view.queryByLabelText("Account login")).toBeNull();
    await i18n.changeLanguage("en");
    expect(view.getByTestId("add-host-method-online-service").textContent).toContain(
      "Online Service",
    );
  });
});
