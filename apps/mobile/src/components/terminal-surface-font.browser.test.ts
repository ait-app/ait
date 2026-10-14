import { Terminal } from "@xterm/xterm";
import React, { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyRootUiFont } from "@/appearance/apply-root-font.web";
import { DEFAULT_TERMINAL_FONT_FAMILY } from "@/terminal/runtime/terminal-font";
import TerminalEmulator from "./terminal-emulator";

vi.mock("expo/dom", () => ({ useDOMImperativeHandle: () => {} }));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    constructor() {
      throw new Error("WebGL unavailable in this DOM renderer regression test");
    }
  },
}));

// Regression: the app-wide interface-font rule declares a high-specificity
// `font-family` over everything under #root. xterm's DOM renderer sets the
// terminal font on `.xterm-rows`, but that rule loses to the ID selector unless
// the terminal surface opts out with `data-pmono`. On machines where WebGL is
// blocked the DOM renderer is used, and a missing marker renders the terminal
// with the proportional UI font, breaking column alignment.
const UI_FONT = "ui-sans-serif";

const mounted = new Set<{ terminal: Terminal; root: HTMLElement }>();
let reactRoot: Root | undefined;

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
  act(() => reactRoot?.unmount());
  reactRoot = undefined;
  for (const { terminal, root } of mounted) {
    terminal.dispose();
    root.remove();
  }
  mounted.clear();
  document.getElementById("root")?.remove();
  document.getElementById("paseo-ui-font")?.remove();
  document.documentElement.style.removeProperty("--paseo-ui-font");
  vi.unstubAllGlobals();
});

describe("terminal surface font resolution", () => {
  it("keeps TerminalEmulator monospace when WebGL is unavailable", async () => {
    // Vitest uses the classic JSX transform for this Expo DOM component.
    vi.stubGlobal("React", React);
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const root = document.createElement("div");
    root.id = "root";
    root.style.width = "800px";
    root.style.height = "400px";
    document.body.appendChild(root);
    applyRootUiFont(UI_FONT);

    const componentHost = document.createElement("div");
    componentHost.style.height = "200px";
    root.appendChild(componentHost);
    reactRoot = createRoot(componentHost);
    act(() => {
      reactRoot?.render(
        createElement(TerminalEmulator, {
          ref: null,
          streamKey: "font-regression",
          supportsTerminalInputModeReplay: false,
          scrollbackLines: 100,
          fontFamily: DEFAULT_TERMINAL_FONT_FAMILY,
        }),
      );
    });

    const plain = document.createElement("div");
    mountInside(root, plain);

    await nextFrame();
    await nextFrame();

    const fontOf = (surface: HTMLElement) => {
      const rows = surface.querySelector<HTMLElement>(".xterm-rows");
      if (!rows) throw new Error("expected xterm rows");
      return getComputedStyle(rows).fontFamily;
    };

    expect(fontOf(componentHost)).toContain("JetBrains Mono");
    // Without the marker the interface-font rule wins; this is the bug the
    // terminal must keep opting out of.
    expect(fontOf(plain)).toBe(UI_FONT);
  });
});
