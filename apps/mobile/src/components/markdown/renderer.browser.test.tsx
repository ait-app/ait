import { isWeb } from "@/constants/platform";
import { WEB_RENDERERS } from "./renderers";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it } from "vitest";
import { MarkdownRenderer } from "./renderer";
import { createAssistantSelectionClipboardContent } from "@/assistant-selection-copy/content.web";

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });

let root: Root | null = null;
let client: QueryClient | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  client?.clear();
  root = null;
  document.body.replaceChildren();
  window.getSelection()?.removeAllRanges();
});

it("renders real Markdown paragraphs, code and tables with selectable copy markup", async () => {
  expect(isWeb).toBe(true);
  expect(WEB_RENDERERS).toBeDefined();
  const host = document.createElement("div");
  host.dataset.testid = "assistant-message";
  document.body.append(host);
  root = createRoot(host);
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await act(async () =>
    root!.render(
      <QueryClientProvider client={client!}>
        <MarkdownRenderer
          text={
            "# Title\n\nFirst **bold** paragraph.\n\nSecond paragraph.\n\n```ts\nconst answer = 42;\n```\n\n| Name | Value |\n| --- | --- |\n| Answer | 42 |"
          }
        />
      </QueryClientProvider>,
    ),
  );
  await expect.poll(() => host.textContent).toContain("const answer = 42;");
  await expect.poll(() => host.querySelectorAll("table").length).toBe(1);
  expect(host.querySelector("strong")?.textContent).toBe("bold");
  expect(host.innerHTML).toContain('data-paseo-markdown-language="ts"');
  const range = document.createRange();
  range.selectNodeContents(host);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  const content = createAssistantSelectionClipboardContent(selection);
  expect(content?.plainText).toContain("First **bold** paragraph.");
  expect(content?.plainText).toContain("const answer = 42;");
  expect(content?.html).toContain("<strong>bold</strong>");
});
