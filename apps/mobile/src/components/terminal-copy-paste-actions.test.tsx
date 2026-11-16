/** @vitest-environment jsdom */
import React from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { i18n } from "@/i18n/i18next";
import { TerminalFloatingCopyAction, TerminalPasteAction } from "./terminal-copy-paste-actions";

afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("en");
});

describe("terminal copy and paste actions", () => {
  it("labels both actions in the active language", async () => {
    await i18n.changeLanguage("zh-CN");
    const view = render(
      <>
        <TerminalPasteAction hasClipboardText onPaste={() => undefined} />
        <TerminalFloatingCopyAction hasSelection onCopy={() => undefined} />
      </>,
    );

    expect(view.getByTestId("terminal-paste").textContent).toBe("粘贴");
    expect(view.getByTestId("terminal-paste").getAttribute("aria-label")).toBe("粘贴");
    expect(view.getByTestId("terminal-copy").textContent).toBe("复制");
    expect(view.getByTestId("terminal-copy").getAttribute("aria-label")).toBe("复制");
  });
});
