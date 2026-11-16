// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { SelectField } from "./select-field";

vi.mock("@/components/ui/combobox", () => ({
  Combobox: ({ emptyText }: { emptyText: string }) => (
    <span data-testid="combobox-empty-text">{emptyText}</span>
  ),
  ComboboxItem: () => null,
}));

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("zh-CN");
});

afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  await i18n.changeLanguage("en");
});

function renderSelectField(loading: boolean) {
  render(
    <SelectField
      label="Model"
      value={null}
      selectedDisplay={null}
      options={[]}
      onChange={() => {}}
      placeholder="Choose"
      emptyText="Nothing here"
      loading={loading}
    />,
  );
  return screen.getByTestId("combobox-empty-text").textContent;
}

describe("SelectField empty text", () => {
  it("shows the translated loading state while options load", () => {
    expect(renderSelectField(true)).toBe(i18n.t("common.states.loading", { lng: "zh-CN" }));
  });

  it("shows the caller's empty text once loading is done", () => {
    expect(renderSelectField(false)).toBe("Nothing here");
  });
});
