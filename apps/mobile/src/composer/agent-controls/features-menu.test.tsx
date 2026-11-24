/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { i18n } from "@/i18n/i18next";
import { AgentFeaturesControl, ComposerFeatureCommandsProvider } from "./features-menu";

const selectCommand = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/use-agent-autocomplete", () => ({
  useAgentAutocomplete: ({
    userInput,
    setUserInput,
  }: {
    userInput: string;
    setUserInput: (value: string) => void;
  }) => ({
    options: userInput
      ? [
          {
            id: "compact",
            label: "/compact",
            type: "provider_command",
            command: {
              name: "compact",
              source: "builtin",
              argumentHint: "",
              description: "Compact conversation",
            },
          },
        ]
      : [],
    onSelectOption: (option: { id: string }) => {
      selectCommand(option.id);
      setUserInput(`/${option.id} `);
    },
    isLoading: false,
  }),
}));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));

const features: AgentFeature[] = [
  { type: "toggle", id: "autoRetry", label: "Automatic retry", value: false },
  {
    type: "select",
    id: "permission",
    label: "Tools",
    value: "ask",
    options: [
      { id: "ask", label: "Ask" },
      { id: "allow", label: "Allow" },
    ],
  },
  {
    type: "select",
    id: "modelRole.advisor",
    label: "Advisor model",
    value: "test/model",
    options: [],
  },
];

beforeEach(async () => {
  vi.stubGlobal("React", React);
  selectCommand.mockClear();
  await i18n.changeLanguage("en");
});
afterEach(cleanup);

it("uses a searchable anchored menu for real functions, excluding model configuration", async () => {
  const onSetFeature = vi.fn();
  render(<AgentFeaturesControl features={features} onSetFeature={onSetFeature} />);
  fireEvent.click(screen.getByTestId("agent-controls-features"));
  expect(await screen.findByPlaceholderText("Search features...")).toBeTruthy();
  expect(screen.queryByText("Advisor model")).toBeNull();
  fireEvent.click(screen.getByText("Automatic retry"));
  expect(onSetFeature).toHaveBeenCalledWith("autoRetry", true);
  fireEvent.click(screen.getByText("Tools"));
  fireEvent.click(await screen.findByText("Allow"));
  expect(onSetFeature).toHaveBeenCalledWith("permission", "allow");
  expect(screen.getByText("Automatic retry")).toBeTruthy();
});

it("loads allowed operations through the shared command menu and inserts the selected command", async () => {
  const setUserInput = vi.fn();
  render(
    <ComposerFeatureCommandsProvider value={{ serverId: "host", agentId: "agent", setUserInput }}>
      <AgentFeaturesControl features={features} onSetFeature={vi.fn()} />
    </ComposerFeatureCommandsProvider>,
  );
  expect(selectCommand).not.toHaveBeenCalled();
  fireEvent.click(screen.getByTestId("agent-controls-features"));
  const search = await screen.findByPlaceholderText("Search features...");
  fireEvent.change(search, { target: { value: "compact" } });
  expect(screen.queryByText("Automatic retry")).toBeNull();
  fireEvent.click(await screen.findByText("/compact"));
  expect(setUserInput).toHaveBeenCalledWith("/compact ");
  expect(screen.queryByPlaceholderText("Search features...")).toBeNull();
});

it("disables the trigger while configuration is unavailable", () => {
  render(<AgentFeaturesControl features={features} disabled />);
  expect(screen.getByTestId("agent-controls-features").getAttribute("aria-disabled")).toBe("true");
});
