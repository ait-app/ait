// @vitest-environment jsdom
import React from "react";
import { act, cleanup, render, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { FileBinaryPreview } from "./binary-preview";
globalThis.React = React;
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (value: string) => value }) }));
const mocks = vi.hoisted(() => ({ openFile: vi.fn(async () => {}) }));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => ({ opener: { openFile: mocks.openFile } }),
}));
vi.mock("expo-sharing", () => ({ shareAsync: vi.fn(async () => {}) }));
vi.mock("@/components/ui/button", () => ({
  Button: ({
    children,
    disabled,
    onPress,
  }: {
    children: React.ReactNode;
    disabled: boolean;
    onPress: () => void;
  }) => (
    <button disabled={disabled} onClick={onPress}>
      {children}
    </button>
  ),
}));
vi.mock("./pdf-preview", () => ({
  canPreviewPdf: () => true,
  FilePdfPreview: ({ uri }: { uri: string }) => <iframe src={uri} />,
}));
afterEach(() => {
  cleanup();
  mocks.openFile.mockClear();
});
const attachment = (fileName: string, mimeType: string) => ({
  id: fileName,
  mimeType,
  fileName,
  storageType: "desktop-file" as const,
  storageKey: `/tmp/desktop-attachments/${fileName}`,
  createdAt: 0,
  byteSize: 10,
});
it("QA: a PDF remains internal until the system-open button is clicked", async () => {
  await act(async () => {
    render(
      <FileBinaryPreview attachment={attachment("report.pdf", "application/pdf")} uri="blob:pdf" />,
    );
  });
  expect(mocks.openFile).not.toHaveBeenCalled();
});
it("QA safety: merely previewing a Windows executable should not invoke the system opener", async () => {
  await act(async () => {
    render(
      <FileBinaryPreview
        attachment={attachment("program.exe", "application/octet-stream")}
        uri="blob:exe"
      />,
    );
  });
  expect(mocks.openFile).not.toHaveBeenCalled();
});

it("opens unsupported files only after an explicit button press", async () => {
  const rendered = render(
    <FileBinaryPreview attachment={attachment("archive.zip", "application/zip")} uri="blob:zip" />,
  );
  expect(mocks.openFile).not.toHaveBeenCalled();
  fireEvent.click(rendered.getByRole("button"));
  await waitFor(() => expect(mocks.openFile).toHaveBeenCalledTimes(1));
});
