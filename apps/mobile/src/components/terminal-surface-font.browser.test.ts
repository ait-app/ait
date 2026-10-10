import { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, it } from "vitest";
import { applyRootUiFont } from "@/appearance/apply-root-font.web";
import { DEFAULT_TERMINAL_FONT_FAMILY } from "@/terminal/runtime/terminal-font";

// Regression: the app-wide interface-font rule declares a high-specificity
// `font-family` over everything under #root. xterm's DOM renderer sets the
// terminal font on `.xterm-rows`, but that rule loses to the ID selector unless
// the terminal surface opts out with `data-pmono`. On machines where WebGL is
// blocked the DOM renderer is used, and a missing marker renders the terminal
// with the proportional UI font, breaking column alignment.
const UI_FONT = "ui-sans-serif";

const mounted = new Set<{ terminal: Terminal; root: HTMLDivElement }>();

function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

function mountInside(container: HTMLElement, surface: HTMLElement): Terminal {
  const host = document.createElement("div");
  container.appendChild(surface);
  surface.appendChild(host);
  const terminal = new Terminal({
    allowProposedApi: true,
    fontFamily: DEFAULT_TERMINAL_FONT_FAMILY,
    fontSize: 12,
    lineHeight: 1.0,
  });
  terminal.open(host);
  terminal.write("AGENTS.md  bins\r\n");
  mounted.add({ terminal, root: surface });
  return terminal;
}

afterEach(() => {
  for (const { terminal, root } of mounted) {
    terminal.dispose();
    root.remove();
  }
  mounted.clear();
  document.getElementById("root")?.remove();
  document.getElementById("paseo-ui-font")?.remove();
  document.documentElement.style.removeProperty("--paseo-ui-font");
});

describe("terminal surface font resolution", () => {
  it("keeps the monospace font on surfaces marked with data-pmono", async () => {
    const root = document.createElement("div");
    root.id = "root";
    document.body.appendChild(root);
    applyRootUiFont(UI_FONT);

    const tagged = document.createElement("div");
    tagged.setAttribute("data-pmono", "");
    mountInside(root, tagged);

    const plain = document.createElement("div");
    mountInside(root, plain);

    await nextFrame();
    await nextFrame();

    const fontOf = (surface: HTMLElement) => {
      const rows = surface.querySelector<HTMLElement>(".xterm-rows");
      if (!rows) throw new Error("expected xterm rows");
      return getComputedStyle(rows).fontFamily;
    };

    expect(fontOf(tagged)).toContain("JetBrains Mono");
    // Without the marker the interface-font rule wins; this is the bug the
    // terminal must keep opting out of.
    expect(fontOf(plain)).toBe(UI_FONT);
  });
});
