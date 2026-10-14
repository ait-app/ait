import { writeFile } from "node:fs/promises";
import path from "node:path";
import { test, expect } from "../support/fixtures";
import { TerminalE2EHarness } from "../support/helpers/terminal-dsl";
import { getTerminalBufferText, waitForTerminalContent } from "../support/helpers/terminal-perf";

const OSC11_CAPTURE_SCRIPT = `
let captured = Buffer.alloc(0);

function finish() {
  process.stdout.write("PASEO_OSC11_CAPTURE:" + JSON.stringify(captured.toString("latin1")) + "\\n");
  process.exit(0);
}

process.stdout.write("\\x1b]11;?\\x07");
if (process.stdin.isTTY) {
  process.stdin.setRawMode(true);
}
process.stdin.resume();
process.stdin.on("data", (chunk) => {
  captured = Buffer.concat([captured, chunk]);
  if (captured.includes(Buffer.from("rgb:"))) {
    finish();
  }
});
setTimeout(finish, 700);
`;

const THEME_MODE_CAPTURE_SCRIPT = `
if (process.stdin.isTTY) process.stdin.setRawMode(true);
let captured = "";
let notifications = 0;
process.stdin.on("data", (chunk) => {
  captured += chunk.toString("latin1");
  const reports = captured.matchAll(/\\x1b\\[\\?997;([12])n/g);
  for (const report of reports) {
    process.stdout.write("PASEO_THEME_MODE:" + ++notifications + ":" + report[1] + "\\n");
  }
  const lastEscape = captured.lastIndexOf("\x1b");
  captured = lastEscape >= 0 && !captured.endsWith("n") ? captured.slice(lastEscape) : "";
});
process.stdin.resume();
process.stdout.write("\x1b[?2031h\x1b[?996n");
setTimeout(() => {
  process.stdout.write("\x1b[?2031l");
  process.exit(0);
}, 30000);
`;

test.describe("Terminal protocol queries", () => {
  let harness: TerminalE2EHarness;

  test.beforeAll(async () => {
    harness = await TerminalE2EHarness.create({ tempPrefix: "terminal-protocol-query-" });
    await writeFile(path.join(harness.tempRepo.path, "osc11-capture.cjs"), OSC11_CAPTURE_SCRIPT);
    await writeFile(
      path.join(harness.tempRepo.path, "theme-mode-capture.cjs"),
      THEME_MODE_CAPTURE_SCRIPT,
    );
  });

  test.afterAll(async () => {
    await harness?.cleanup();
  });

  for (const theme of [
    { name: "light", background: "rgb:ffff/ffff/ffff" },
    { name: "dark", background: "rgb:1818/1b1b/1a1a" },
  ]) {
    test(`answers OSC 11 with the displayed ${theme.name} background`, async ({ page }) => {
      await page.addInitScript((theme) => {
        localStorage.setItem("@paseo:app-settings", JSON.stringify({ theme }));
      }, theme.name);
      const terminalInstance = await harness.createTerminal({ name: "osc11-query" });
      try {
        await harness.openTerminal(page, { terminalId: terminalInstance.id });
        await harness.setupPrompt(page);

        const terminal = harness.terminalSurface(page);
        await terminal.pressSequentially("node osc11-capture.cjs\n", { delay: 0 });

        await waitForTerminalContent(page, (text) => text.includes("PASEO_OSC11_CAPTURE:"), 10_000);
        await page.waitForTimeout(500);

        const text = await getTerminalBufferText(page);

        expect(text).toContain(theme.background);
        expect(text).not.toContain("rgb:0b0b/0b0b/0b0b");
      } finally {
        await harness.killTerminal(terminalInstance.id);
      }
    });
  }

  test("notifies a running TUI when the system color scheme changes", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light" });
    await page.addInitScript(() => {
      localStorage.setItem("@paseo:app-settings", JSON.stringify({ theme: "auto" }));
    });
    const terminal = await harness.createTerminal({ name: "theme-mode" });
    try {
      await harness.openTerminal(page, { terminalId: terminal.id });
      await harness.setupPrompt(page);
      await harness
        .terminalSurface(page)
        .pressSequentially("node theme-mode-capture.cjs\n", { delay: 0 });
      await expect.poll(() => getTerminalBufferText(page)).toContain("PASEO_THEME_MODE:1:2");
      await page.evaluate(() => {
        const win = window as Window & { __paseoTerminal?: unknown; __themeTestTerminal?: unknown };
        win.__themeTestTerminal = win.__paseoTerminal;
      });

      await page.emulateMedia({ colorScheme: "dark" });
      await expect.poll(() => getTerminalBufferText(page)).toContain("PASEO_THEME_MODE:2:1");
      expect(
        await page.evaluate(() => {
          const win = window as Window & {
            __paseoTerminal?: unknown;
            __themeTestTerminal?: unknown;
          };
          return win.__themeTestTerminal === win.__paseoTerminal;
        }),
      ).toBe(true);

      await page.emulateMedia({ colorScheme: "light" });
      await expect.poll(() => getTerminalBufferText(page)).toContain("PASEO_THEME_MODE:3:2");
    } finally {
      await harness.killTerminal(terminal.id);
    }
  });
});
