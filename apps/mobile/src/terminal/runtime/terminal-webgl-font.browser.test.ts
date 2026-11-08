import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { afterEach, describe, expect, it, vi } from "vitest";

const GLYPHS = [
  { text: "Ω", attributes: "0", style: "normal", weight: "normal" },
  { text: "Ψ", attributes: "1", style: "normal", weight: "bold" },
  { text: "Φ", attributes: "0;3", style: "italic", weight: "normal" },
];

interface GlyphDraw {
  font: string;
  width: number;
}

let terminal: Terminal | undefined;
let host: HTMLDivElement | undefined;

afterEach(() => {
  terminal?.dispose();
  terminal = undefined;
  host?.remove();
  host = undefined;
  vi.restoreAllMocks();
});

function captureGlyphsWithStrictFontParsing(): Map<string, GlyphDraw> {
  const prototype = CanvasRenderingContext2D.prototype;
  const fontSetter = Object.getOwnPropertyDescriptor(prototype, "font")?.set;
  if (!fontSetter) throw new Error("Canvas font setter is unavailable");
  const fillText = prototype.fillText;
  const draws = new Map<string, GlyphDraw>();

  // Electron 45.2.3 rejects leading whitespace and retains the previous font.
  // Model only that parser regression; keep the real canvas and WebGL renderer.
  // https://github.com/electron/electron/issues/55616
  vi.spyOn(prototype, "font", "set").mockImplementation(function (
    this: CanvasRenderingContext2D,
    value: string,
  ) {
    if (!/^\s/.test(value)) fontSetter.call(this, value);
  });
  vi.spyOn(prototype, "fillText").mockImplementation(function (
    this: CanvasRenderingContext2D,
    text: string,
    x: number,
    y: number,
    maxWidth?: number,
  ) {
    if (GLYPHS.some((glyph) => glyph.text === text)) {
      draws.set(text, { font: this.font, width: this.measureText(text).width });
    }
    fillText.call(this, text, x, y, maxWidth);
  });
  return draws;
}

async function expectGlyphFonts(
  draws: Map<string, GlyphDraw>,
  fontSize: number,
  fontFamily: string,
): Promise<void> {
  await expect.poll(() => GLYPHS.every((glyph) => draws.has(glyph.text))).toBe(true);
  const expected = document.createElement("canvas").getContext("2d");
  if (!expected) throw new Error("Canvas context is unavailable");

  for (const glyph of GLYPHS) {
    expected.font = `${glyph.style} ${glyph.weight} ${fontSize * window.devicePixelRatio}px ${fontFamily}`;
    expect(draws.get(glyph.text)?.font, `${glyph.style} ${glyph.weight} glyph`).toBe(expected.font);
    expect(draws.get(glyph.text)?.width).toBeCloseTo(expected.measureText(glyph.text).width, 3);
  }
}

function cellHeightAtDeviceScale(element: HTMLElement, instance: Terminal): number {
  const screen = element.querySelector<HTMLElement>(".xterm-screen");
  const canvas = screen?.querySelector<HTMLCanvasElement>("canvas:not(.xterm-link-layer)");
  if (!screen || !canvas) throw new Error("Expected the WebGL terminal canvas");
  const bounds = screen.getBoundingClientRect();
  const dpr = window.devicePixelRatio;
  expect(canvas.width / instance.cols / dpr).toBeCloseTo(bounds.width / instance.cols, 1);
  expect(canvas.height / instance.rows / dpr).toBeCloseTo(bounds.height / instance.rows, 1);
  return bounds.height / instance.rows;
}

describe("terminal WebGL font rendering", () => {
  it("paints regular, bold and italic glyphs at the selected size after font changes", async () => {
    const draws = captureGlyphsWithStrictFontParsing();
    host = document.createElement("div");
    host.style.width = "800px";
    host.style.height = "300px";
    document.body.appendChild(host);
    terminal = new Terminal({
      allowProposedApi: true,
      cols: 40,
      rows: 8,
      fontSize: 12,
      fontFamily: "monospace",
      lineHeight: 1,
    });
    terminal.open(host);
    const addon = new WebglAddon();
    terminal.loadAddon(addon);
    // Non-ASCII glyphs avoid the atlas's asynchronous ASCII warm-up cache.
    const text = GLYPHS.map((glyph) => `\x1b[${glyph.attributes}m${glyph.text}`).join("");
    await new Promise<void>((resolve) => terminal?.write(`${text}\x1b[0m\r\n`, resolve));

    await expectGlyphFonts(draws, 12, "monospace");
    expect(addon.textureAtlas).toBeInstanceOf(HTMLCanvasElement);
    const initialCellHeight = cellHeightAtDeviceScale(host, terminal);

    draws.clear();
    terminal.options.fontFamily = '"Courier New", monospace';
    terminal.options.fontSize = 18;
    await new Promise<void>((resolve) => terminal?.write(`${text}\x1b[0m`, resolve));

    await expectGlyphFonts(draws, 18, '"Courier New", monospace');
    expect(cellHeightAtDeviceScale(host, terminal)).toBeGreaterThan(initialCellHeight);
  });
});
