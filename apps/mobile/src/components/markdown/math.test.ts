import { parseMarkdown } from "react-native-enriched-markdown/src/web/parseMarkdown";
import type { ASTNode } from "react-native-enriched-markdown/src/web/types";
import { describe, expect, it } from "vitest";
import { darkTheme, lightTheme } from "@/styles/theme";
import { prepareMarkdownSource } from "./source";
import { createMarkdownStyles } from "./styles";
import { TAI_MATH_ANSWER, TAI_P95_FORMULA } from "./math-fixture";

function mathNodes(node: ASTNode): ASTNode[] {
  return [
    ...(node.type.startsWith("LatexMath") ? [node] : []),
    ...(node.children?.flatMap(mathNodes) ?? []),
  ];
}

async function parse(source: string) {
  return parseMarkdown(prepareMarkdownSource(source).render(), { latexMath: true });
}

describe("shared Markdown math", () => {
  it("preserves the Tai p95 expression through source adaptation and parses display/inline math", async () => {
    const source = TAI_MATH_ANSWER;
    expect(prepareMarkdownSource(source).render()).toBe(source);
    const math = mathNodes(await parse(source));
    expect(math.map((node) => node.type)).toEqual(["LatexMathDisplay", "LatexMathInline"]);
    expect(JSON.stringify(math)).toContain(String.raw`\\text{sorted}`);
  });

  it("leaves inline/fenced code, escaped currency and an unclosed formula as text", async () => {
    for (const source of [
      "`$n=20$`\n\n```tex\n$$x^2$$\n```",
      String.raw`Cost: \$5.00 and \$10.00.`,
      "Cost: $12.50.",
      `Before\n\n$$${TAI_P95_FORMULA}`,
    ]) {
      expect(mathNodes(await parse(source)), source).toEqual([]);
    }
  });

  it.each([lightTheme, darkTheme])("uses the prose theme size, color and block gap", (theme) => {
    for (const compact of [false, true]) {
      const { markdownStyle: style } = createMarkdownStyles(theme, { compact, blockGap: 12 });
      expect(style.math?.fontSize).toBe(style.paragraph?.fontSize);
      expect(style.math?.color).toBe(theme.colors.foreground);
      expect(style.inlineMath?.color).toBe(theme.colors.foreground);
      expect(style.math?.marginBottom).toBe(12);
    }
  });
});
