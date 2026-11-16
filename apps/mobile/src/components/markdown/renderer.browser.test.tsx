import { isWeb } from "@/constants/platform";
import { WEB_RENDERERS } from "./renderers";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import { MarkdownRenderer } from "./renderer";
import { createAssistantSelectionClipboardContent } from "@/assistant-selection-copy/content.web";
import { TAI_MATH_ANSWER, TAI_P95_FORMULA } from "./math-fixture";

vi.mock("react-native-enriched-markdown/src/web/parseMarkdown", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("react-native-enriched-markdown/src/web/parseMarkdown")>();
  return {
    ...actual,
    parseMarkdown: (...args: Parameters<typeof actual.parseMarkdown>) =>
      args[0] === "MATH_PARSE_FAILURE_FIXTURE"
        ? Promise.reject(new Error("Injected parser failure"))
        : actual.parseMarkdown(...args),
    parseMarkdownSync: (...args: Parameters<typeof actual.parseMarkdownSync>) => {
      if (args[0] === "MATH_PARSE_FAILURE_FIXTURE") throw new Error("Injected parser failure");
      return actual.parseMarkdownSync(...args);
    },
  };
});

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

async function renderMath(text: string, streaming = false) {
  const host =
    document.querySelector<HTMLElement>("[data-testid=math-message]") ??
    document.body.appendChild(document.createElement("div"));
  host.dataset.testid = "math-message";
  root ??= createRoot(host);
  client ??= new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await act(async () =>
    root!.render(
      <QueryClientProvider client={client!}>
        <MarkdownRenderer text={text} streaming={streaming} />
      </QueryClientProvider>,
    ),
  );
  return host;
}

it("typesets the real Tai display formula and inline follow-up using local MathML", async () => {
  const host = await renderMath(TAI_MATH_ANSWER);
  await expect.poll(() => host.querySelectorAll("[role=math] math").length).toBe(2);
  const display = host.querySelector<HTMLElement>("[role=math]")!;
  expect(display.getAttribute("aria-label")).toBe(TAI_P95_FORMULA);
  expect(display.querySelector("math")?.getAttribute("display")).toBe("block");
  expect(display.querySelector("mtext")?.textContent).toBe("sorted");
  expect(display.querySelectorAll("mo").length).toBeGreaterThan(4);
  expect(getComputedStyle(display).fontSize).toBe(
    getComputedStyle(host.querySelector("p")!).fontSize,
  );
  expect(getComputedStyle(display).color).toBe(getComputedStyle(host.querySelector("p")!).color);
  expect(document.querySelector('script[src*="katex"],link[href*="cdn"]')).toBeNull();
  host.style.width = "330px";
  expect(display.clientWidth).toBeLessThanOrEqual(330);
  expect(getComputedStyle(display).overflowX).toBe("auto");
  await renderMath(`$$${TAI_P95_FORMULA}+${TAI_P95_FORMULA}$$`);
  await expect.poll(() => host.querySelectorAll("math").length).toBe(1);
  const longFormula = host.querySelector<HTMLElement>("[role=math]")!;
  expect(longFormula.clientWidth).toBeLessThanOrEqual(330);
  expect(longFormula.scrollWidth).toBeGreaterThan(longFormula.clientWidth);
});

it("keeps code and incomplete streaming formulas intact, then typesets the completed expression", async () => {
  const literal = "`$n=20$`\n\n```tex\n$$x^2$$\n```\n\nCost: \\$5 and \\$10.";
  const host = await renderMath(`${literal}\n\n$$\\frac{1}{`, true);
  await expect.poll(() => host.querySelectorAll("code").length).toBeGreaterThan(0);
  expect(host.querySelectorAll("math").length).toBe(0);
  expect(host.textContent).toContain("$n=20$");
  expect(host.textContent).toContain("$$x^2$$");
  expect(host.textContent).toContain("$5 and $10");
  await renderMath(`${literal}\n\n$$\\frac{1}{2}$$`, true);
  await expect.poll(() => host.querySelectorAll("math mfrac").length).toBe(1);
});

it("renders invalid or untrusted TeX without injecting HTML, scripts or remote images", async () => {
  const source = String.raw`$\htmlClass{pwned}{x}+\href{javascript:alert(1)}{bad}+\includegraphics{https://invalid.example/pixel}$

$$\definitelyInvalidCommand{x}$$`;
  const host = await renderMath(source);
  await expect.poll(() => host.querySelectorAll("[role=math] math").length).toBe(2);
  expect(host.querySelector(".pwned,script,img,a[href^='javascript:']")).toBeNull();
  expect(host.querySelectorAll("math mtext").length).toBeGreaterThan(0);
});

it("recovers math loading after a parser failure when a valid source arrives", async () => {
  const host = await renderMath("MATH_PARSE_FAILURE_FIXTURE", true);
  await expect
    .poll(() => host.querySelector("pre")?.textContent)
    .toBe("MATH_PARSE_FAILURE_FIXTURE");
  await renderMath(TAI_MATH_ANSWER, true);
  await expect.poll(() => host.querySelectorAll("[role=math] math").length).toBe(2);
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
