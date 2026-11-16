// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { Autocomplete } from "./autocomplete";

beforeEach(() => {
  vi.stubGlobal("React", React);
});

afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  await i18n.changeLanguage("en");
});

function renderError() {
  render(<Autocomplete options={[]} selectedIndex={-1} onSelect={() => {}} errorMessage="boom" />);
}

describe("Autocomplete error row", () => {
  it("keeps the English prefix", () => {
    renderError();
    expect(screen.getByText("Error: boom")).toBeTruthy();
  });

  it("translates the prefix around the message", async () => {
    await i18n.changeLanguage("zh-CN");
    renderError();
    const expected = i18n.t("common.errors.withMessage", { message: "boom", lng: "zh-CN" });
    expect(expected).not.toBe("Error: boom");
    expect(screen.getByText(expected)).toBeTruthy();
  });
});
