/** @vitest-environment jsdom */
import "@/i18n/i18next";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useToastHost } from "./toast-host";

afterEach(cleanup);

it("dismisses only the toast created by the returned callback", () => {
  const { result } = renderHook(useToastHost);
  let dismiss: (() => void) | void;
  act(() => {
    dismiss = result.current.api.show("Reconnecting", { variant: "loading" });
  });
  expect(result.current.toast?.content).toBe("Reconnecting");
  act(() => dismiss?.());
  expect(result.current.toast).toBeNull();

  act(() => {
    dismiss = result.current.api.show("Reconnecting", { variant: "loading" });
    result.current.api.show("Copied");
  });
  act(() => dismiss?.());
  expect(result.current.toast?.content).toBe("Copied");
});
