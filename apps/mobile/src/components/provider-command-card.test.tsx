// @vitest-environment jsdom
import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ProviderCommandTimelineItemSchema } from "@ait/protocol/messages";
import { en } from "@/i18n/resources/en";
import { ProviderCommandCard } from "./provider-command-card";
import fixture from "../../../../crates/provider/tests/fixtures/dsh_goal_control.json";
import exportFixture from "../../../../crates/provider/tests/fixtures/dsh_export_control.json";

const download = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/use-file-download", () => ({ useFileDownload: vi.fn(() => download) }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => {
      const value = key
        .split(".")
        .reduce<unknown>((node, part) => (node as Record<string, unknown>)?.[part], en);
      return String(value ?? key).replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
        String(values?.[name] ?? ""),
      );
    },
  }),
}));
vi.mock("@/components/ui/loading-spinner", () => ({
  LoadingSpinner: () => <span role="progressbar" />,
}));

beforeEach(() => {
  vi.stubGlobal("React", React);
  download.mockClear();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
const result = ProviderCommandTimelineItemSchema.parse(fixture);

it("downloads the delivered native archive through the shared host file pipeline", () => {
  const result = ProviderCommandTimelineItemSchema.parse(exportFixture);
  render(<ProviderCommandCard result={result} active={false} serverId="server" />);
  expect(screen.getByText("session.zip")).toBeTruthy();
  fireEvent.click(screen.getByText(en.panels.file.downloadFile));
  expect(download).toHaveBeenCalledExactlyOnceWith({
    fileName: "session.zip",
    path: "session.zip",
  });
});

it("presents native goal facts, folds raw output, and submits the advertised resume action", async () => {
  const onCommand = vi.fn(async () => undefined);
  render(<ProviderCommandCard result={result} active onCommand={onCommand} />);
  expect(screen.getByText("Paused")).toBeTruthy();
  expect(screen.getByText("Keep the objective intact")).toBeTruthy();
  expect(screen.queryByText(result.text)).toBeNull();
  fireEvent.click(screen.getByText("Resume"));
  await waitFor(() => expect(onCommand).toHaveBeenCalledExactlyOnceWith("/goal resume"));
  fireEvent.click(screen.getByText("Details"));
  expect(screen.getByText(result.text)).toBeTruthy();
});

it("preserves edited objectives and keeps historical cards read-only", async () => {
  const onCommand = vi.fn(async () => undefined);
  const rendered = render(<ProviderCommandCard result={result} active onCommand={onCommand} />);
  fireEvent.click(screen.getByText("Edit goal"));
  fireEvent.change(screen.getByLabelText("Edit goal objective"), {
    target: { value: "  replacement\nobjective  " },
  });
  fireEvent.click(screen.getByText("Save"));
  await waitFor(() =>
    expect(onCommand).toHaveBeenCalledExactlyOnceWith("/goal edit   replacement\nobjective  "),
  );
  rendered.rerender(<ProviderCommandCard result={result} active={false} onCommand={onCommand} />);
  expect(screen.queryByText("Resume")).toBeNull();
  expect(screen.queryByText("Clear")).toBeNull();
});
