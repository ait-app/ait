import { parseMarkdown } from "react-native-enriched-markdown/src/web/parseMarkdown";
import type { ASTNode } from "react-native-enriched-markdown/src/web/types";
import { describe, expect, it } from "vitest";
import { normalizeCjkEmphasis } from "./cjk-emphasis";
import { prepareMarkdownSource } from "./source";

const FIXED = [
  [
    "**CloudFront 的“按量计费免费额度”是 1TB/月；另一种“固定套餐 Free”只有 100GB/月。**选购时别混在一起。",
    "**CloudFront 的“按量计费免费额度”是 1TB/月；另一种“固定套餐 Free”只有 100GB/月**。选购时别混在一起。",
  ],
  ["另一种**“固定套餐”**只有 100GB。", "另一种“**固定套餐**”只有 100GB。"],
  [
    "**如果你的境外 CDN 用量在 1TB/月以内，我建议先用 CloudFront 按量模式验证这套分流。**它有免费额度。",
    "**如果你的境外 CDN 用量在 1TB/月以内，我建议先用 CloudFront 按量模式验证这套分流**。它有免费额度。",
  ],
  ["**注意：**这里", "**注意**：这里"],
  ["**注意:**这里", "**注意**:这里"],
  ["**太好了🎉**真的", "**太好了**🎉真的"],
  ["**甲**乙**“丙”**丁", "**甲**乙“**丙**”丁"],
  ["> **引用。**内容", "> **引用**。内容"],
  ["- **列表。**内容\n- 下一项", "- **列表**。内容\n- 下一项"],
  ["## **标题。**说明", "## **标题**。说明"],
  [
    "| **名称：**说明 | 值 |\n| --- | --- |\n| a | b |",
    "| **名称**：说明 | 值 |\n| --- | --- |\n| a | b |",
  ],
  ["**跨行\n内容。**之后", "**跨行\n内容**。之后"],
  ["1. **甲。**乙\n2. **丙。**丁", "1. **甲**。乙\n2. **丙**。丁"],
  // `2.` cannot interrupt a paragraph, so the opener is still pending.
  ["文字 **甲\n2. 乙。**丙", "文字 **甲\n2. 乙**。丙"],
  // A table header must start its block; this pipe row is paragraph text.
  ["说明\n| **名称：**说明 |\n| --- |", "说明\n| **名称**：说明 |\n| --- |"],
  ["[**链接。**文字](https://x.com/a。**b)", "[**链接**。文字](https://x.com/a。**b)"],
  ["见 `**代码。**中` 后，**说明。**内容", "见 `**代码。**中` 后，**说明**。内容"],
] as const;

const UNCHANGED = [
  "你之前要的 **1TB 沪日专线流量**，和这里的 **1TB CDN 出站流量**分别计费。",
  "你好，**世界**",
  "你好，**世界**。再见",
  // An opener left in an earlier block must not turn a later opener into a closer.
  "**未闭合\n\n你好，**世界**",
  "**未闭合\n- 你好，**世界**",
  "**未闭合\n# 你好，**世界**",
  "**未闭合\n> 你好，**世界**",
  "**未闭合\n```\n代码\n```\n你好，**世界**",
  "**Note:** text",
  "**bold**text",
  "Plain **Note.**Next without CJK",
  "用 `**价格。**中` 表示",
  "用 ``**价格。** ` 中`` 表示",
  "```md\n**价格。**中\n```",
  "~~~\n**价格。**中\n~~~",
  "说明：\n\n    **价格。**中",
  "\\*\\*价格。\\*\\*中",
  "**价格。\\**中",
  '见 <https://x.com/**a。**中> 与 <b title="**甲。**乙">',
  "[文字](https://x.com/**a。**中)",
  "https://x.com/**a。**中",
  "**价格 。**中",
  "价格**“ 固定”**只有",
  // A single `*` keeps its literal spelling; only `**` is adapted.
  "*注意：*这里",
  // `***` splits into strong and emphasis; MD4C keeps its own pairing.
  "***注意：***这里",
  // A failed opener without a later closer stays literal and in place.
  "另一种**“固定套餐”只有",
  // Delimiters after an unclosed bracket never pair with ones before it.
  "**甲 [乙。**丙",
  "流式输出：**",
  "**",
  "流式**",
  "```\n未结束的 **代码。**中",
];

async function strongTexts(markdown: string): Promise<string[]> {
  const tree = await parseMarkdown(markdown, { hardSoftBreaks: true, latexMath: false });
  const strong: string[] = [];
  const text = (node: ASTNode): string =>
    node.type === "Text" ? (node.content ?? "") : (node.children ?? []).map(text).join("");
  const visit = (node: ASTNode) => {
    if (node.type === "Strong") strong.push(text(node));
    node.children?.forEach(visit);
  };
  visit(tree);
  return strong;
}

describe("CJK emphasis delimiter adaptation", () => {
  it.each(FIXED)("moves delimiters made non-flanking by CJK punctuation: %s", (source, fixed) => {
    expect(normalizeCjkEmphasis(source)).toBe(fixed);
    expect(normalizeCjkEmphasis(fixed)).toBe(fixed);
  });

  it.each(UNCHANGED)("keeps flanking, protected and unpaired runs: %s", (source) => {
    expect(normalizeCjkEmphasis(source)).toBe(source);
  });

  it("lets MD4C pair every adapted delimiter", async () => {
    for (const [source, fixed] of FIXED) {
      expect((await strongTexts(fixed)).length, fixed).toBeGreaterThan(
        (await strongTexts(source)).length,
      );
    }
    expect(await strongTexts(FIXED[1][1])).toEqual(["固定套餐"]);
  });

  it("reaches the renderer through the shared source adaptation", () => {
    const prepared = prepareMarkdownSource("另一种**“固定套餐”**只有 100GB。");
    expect(prepared.render()).toBe("另一种“**固定套餐**”只有 100GB。");
  });

  it("is stable across every streaming prefix", () => {
    const source = `${FIXED.map(([text]) => text).join("\n\n")}\n\n${UNCHANGED.join("\n\n")}`;
    for (let end = 0; end <= source.length; end++) {
      const prefix = source.slice(0, end);
      const adapted = normalizeCjkEmphasis(prefix);
      expect(adapted.length).toBe(prefix.length);
      expect(normalizeCjkEmphasis(adapted), `prefix ${end}`).toBe(adapted);
    }
  });

  it("bolds every generated CJK group that MD4C leaves literal", async () => {
    const random = generator(11);
    const pick = <T>(items: readonly T[]) => items[Math.floor(random() * items.length)];
    const marks = ["", "", "。", "“", "”", "：", "，", "！", "（", "）", "、", "🎉", ":", "."];
    const separators = [" ", "，", "。", "中", "和", "：", "“", "”"];
    for (let sample = 0; sample < 1500; sample++) {
      const words: string[] = [];
      let source = pick(["", "中", "，", "说明"]);
      const groups = 1 + Math.floor(random() * 4);
      for (let group = 0; group < groups; group++) {
        const word = pick(["甲", "乙丙", "固定套餐", "ストア", "한국어"]);
        words.push(word);
        if (group > 0) source += pick(separators);
        source += `**${pick(marks)}${word}${pick(marks)}**`;
      }
      source += pick(["", "中", "。", "选购"]);
      const strong = await strongTexts(normalizeCjkEmphasis(source));
      expect(strong.length, source).toBe(words.length);
      strong.forEach((text, index) => expect(text, source).toContain(words[index]));
    }
  });

  it("only reorders characters, idempotently, on arbitrary input", async () => {
    const random = generator(7);
    // Weighted towards the delimiter and CJK adjacencies the adaptation targets.
    const pieces = "** ** ** 中 中 文 。 “ ” ： * a ` [ ](u) \\".split(" ");
    pieces.push(" ", "\n", "> ", "- ");
    let changed = 0;
    for (let sample = 0; sample < 3000; sample++) {
      let source = "";
      const count = 4 + Math.floor(random() * 24);
      for (let index = 0; index < count; index++) {
        source += pieces[Math.floor(random() * pieces.length)];
      }
      const adapted = normalizeCjkEmphasis(source);
      expect([...adapted].sort().join("")).toBe([...source].sort().join(""));
      expect(normalizeCjkEmphasis(adapted), source).toBe(adapted);
      if (adapted === source) continue;
      changed++;
      // Every committed move pairs a run that MD4C then renders as strong.
      expect((await strongTexts(adapted)).length, source).toBeGreaterThan(0);
    }
    expect(changed).toBeGreaterThan(10);
  });
});

function generator(seed: number) {
  return () => {
    seed = (seed * 48271) % 2147483647;
    return seed / 2147483647;
  };
}
