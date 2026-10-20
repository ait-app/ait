import type { ParsedDiffFile } from "@ait/protocol/messages";
import { Skia } from "@shopify/react-native-skia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { buildDiffDocumentModel } from "./model";
import { createNativePaints, paintNativeViewport } from "./paint.native";
import {
  createNativeTextLayoutStore,
  disposeNativeTextLayout,
  prepareNativeTextLayout,
} from "./text.native";
import type { DiffPalette } from "./types";

const graphics = vi.hoisted(() => {
  const rendered: string[] = [];
  const paragraphs: { text: string; dispose: ReturnType<typeof vi.fn> }[] = [];
  const font = {
    getGlyphIDs: (text: string) =>
      Array.from(text, (character) => (character.charCodeAt(0) < 128 ? 1 : 0)),
    getTextWidth: (text: string) => text.length * 8,
    dispose: vi.fn(),
  };
  const canvas = {
    drawRect: vi.fn(),
    drawText(text: string) {
      rendered.push(
        Array.from(text, (character) => (character.charCodeAt(0) < 128 ? character : "□")).join(""),
      );
    },
  };
  return { rendered, paragraphs, font, canvas };
});

vi.mock("@shopify/react-native-skia", () => ({
  ClipOp: {},
  PaintStyle: {},
  StrokeCap: {},
  StrokeJoin: {},
  listFontFamilies: () => ["Menlo"],
  matchFont: () => graphics.font,
  Skia: {
    Color: (color: string) => color,
    Paint: () => ({ setColor: vi.fn() }),
    XYWHRect: (x: number, y: number, width: number, height: number) => ({ x, y, width, height }),
    Surface: { MakeOffscreen: () => ({ getCanvas: () => graphics.canvas }) },
    ParagraphBuilder: {
      Make: () => {
        let text = "";
        const builder = {
          pushStyle: () => builder,
          addText(value: string) {
            text += value;
            return builder;
          },
          pop: () => builder,
          build() {
            const paragraph = {
              text,
              layout: vi.fn(),
              getHeight: () => 18,
              paint: () => graphics.rendered.push(text),
              dispose: vi.fn(),
            };
            graphics.paragraphs.push(paragraph);
            return paragraph;
          },
        };
        return builder;
      },
    },
  },
}));

const palette: DiffPalette = {
  surface: "#fff",
  headerSurface: "#eee",
  border: "#ddd",
  foreground: "#111",
  foregroundMuted: "#555",
  addition: "green",
  deletion: "red",
  additionBackground: "#efe",
  deletionBackground: "#fee",
  emptyBackground: "#eee",
  selection: "blue",
  headerActiveSurface: "#ddd",
  headerBorder: "#ccc",
  statusSuccess: "green",
  statusDanger: "red",
  statusWarning: "orange",
  syntax: {},
};

function file(path: string, status: "binary" | "too_large"): ParsedDiffFile {
  return { path, status, isNew: true, isDeleted: false, additions: 0, deletions: 0, hunks: [] };
}

function model(files: ParsedDiffFile[], binary = "二进制文件", tooLarge = "文件过大") {
  return buildDiffDocumentModel({
    files,
    collapsedFilePaths: new Set(),
    layout: "unified",
    wrapLines: false,
    viewportWidth: 400,
    typography: { family: "Menlo", size: 12, lineHeight: 18 },
    measureText: { measure: (text) => text.length * 8 },
    palette,
    labels: { binary, tooLarge },
  });
}

function store() {
  return createNativeTextLayoutStore({
    configuredFamily: "Menlo",
    fontSize: 12,
    lineHeight: 18,
    palette,
  });
}

beforeEach(() => {
  graphics.rendered.length = 0;
  graphics.paragraphs.length = 0;
  vi.clearAllMocks();
});

describe("native diff status text", () => {
  const cases: { status: "binary" | "too_large"; label: string }[] = [
    { status: "binary", label: "二进制文件" },
    { status: "too_large", label: "文件过大，无法显示差异" },
    { status: "binary", label: "バイナリファイル" },
    { status: "binary", label: "ملف ثنائي" },
    { status: "binary", label: "Binary file" },
  ];

  it.each(cases)("renders $status status as readable text: $label", ({ status, label }) => {
    const document = model([file("image.png", status)], label, label);
    const textStore = store();
    const textLayout = prepareNativeTextLayout(textStore, document);
    const surface = Skia.Surface.MakeOffscreen(400, 400);
    if (!surface) throw new Error("Missing test surface");
    paintNativeViewport({
      canvas: surface.getCanvas(),
      model: document,
      viewportWidth: 400,
      viewportHeight: 400,
      scrollTop: 0,
      horizontalOffsets: {},
      textLayout,
      paints: createNativePaints(palette),
    });
    expect(graphics.rendered).toEqual([label]);
    disposeNativeTextLayout(textStore);
  });

  it("reuses identical status text across files and repeated preparation", () => {
    const document = model([file("first.png", "binary"), file("second.png", "binary")]);
    const textStore = store();
    prepareNativeTextLayout(textStore, document);
    prepareNativeTextLayout(textStore, document);
    expect(graphics.paragraphs.map((paragraph) => paragraph.text)).toEqual(["二进制文件"]);
    disposeNativeTextLayout(textStore);
    expect(graphics.paragraphs[0]?.dispose).toHaveBeenCalledTimes(1);
  });

  it("releases status text outside the prepared window and on disposal", () => {
    const document = model([file("image.png", "binary"), file("large.txt", "too_large")]);
    const [first, second] = document.rows;
    if (!first || !second) throw new Error("Missing test status rows");
    const textStore = store();
    prepareNativeTextLayout(textStore, document, { top: first.top, height: first.height });
    expect(graphics.paragraphs.map((paragraph) => paragraph.text)).toEqual(["二进制文件"]);
    prepareNativeTextLayout(textStore, document, { top: second.top, height: second.height });
    expect(graphics.paragraphs[0]?.dispose).toHaveBeenCalledTimes(1);
    expect(graphics.paragraphs[1]?.text).toBe("文件过大");
    disposeNativeTextLayout(textStore);
    expect(graphics.paragraphs[0]?.dispose).toHaveBeenCalledTimes(1);
    expect(graphics.paragraphs[1]?.dispose).toHaveBeenCalledTimes(1);
    expect(graphics.font.dispose).toHaveBeenCalledTimes(1);
  });
});
