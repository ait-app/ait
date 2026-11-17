import { describe, expect, test } from "vitest";

import { BROWSER_AUTOMATION_COMMAND_NAMES } from "./browser-automation/rpc-schemas.js";
import { BrowserAutomationHostCapabilitySchema } from "./browser-automation/capabilities.js";
import { CLIENT_CAPS } from "./client-capabilities.js";
import {
  MutableDaemonConfigPatchSchema,
  MutableDaemonConfigSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
} from "./messages.js";

function parseBrowserCapabilities(input: unknown) {
  const capabilities =
    typeof input === "object" && input !== null ? (input as Record<string, unknown>) : undefined;
  const browserHost = capabilities?.[CLIENT_CAPS.browserHost];
  return {
    capabilities:
      browserHost === undefined
        ? undefined
        : { [CLIENT_CAPS.browserHost]: BrowserAutomationHostCapabilitySchema.parse(browserHost) },
  };
}

describe("browser automation protocol integration", () => {
  const browserId = "11111111-1111-4111-8111-111111111111";

  test("browser host capability parses supported commands", () => {
    expect(
      parseBrowserCapabilities({
        [CLIENT_CAPS.browserHost]: {
          supportedCommands: [...BROWSER_AUTOMATION_COMMAND_NAMES],
          hostKind: "desktop app",
        },
      }).capabilities,
    ).toMatchObject({
      [CLIENT_CAPS.browserHost]: {
        supportedCommands: [...BROWSER_AUTOMATION_COMMAND_NAMES],
        hostKind: "desktop app",
      },
    });
  });

  test("browser host capability requires at least one supported command", () => {
    expect(() =>
      parseBrowserCapabilities({
        [CLIENT_CAPS.browserHost]: {
          supportedCommands: [],
          hostKind: "desktop app",
        },
      }),
    ).toThrow();

    expect(() =>
      parseBrowserCapabilities({
        [CLIENT_CAPS.browserHost]: {},
      }),
    ).toThrow();

    expect(() =>
      parseBrowserCapabilities({
        [CLIENT_CAPS.browserHost]: {
          supportedCommands: ["future_command"],
        },
      }),
    ).toThrow();
  });

  test("browser host capability ignores unknown future commands when known commands remain", () => {
    expect(
      parseBrowserCapabilities({
        [CLIENT_CAPS.browserHost]: {
          supportedCommands: ["list_tabs", "future_command", "list_tabs"],
          hostKind: "desktop app",
        },
      }).capabilities,
    ).toMatchObject({
      [CLIENT_CAPS.browserHost]: {
        supportedCommands: ["list_tabs"],
        hostKind: "desktop app",
      },
    });
  });

  test("browser host capability accepts new tool commands as supported commands", () => {
    expect(
      parseBrowserCapabilities({
        [CLIENT_CAPS.browserHost]: {
          supportedCommands: ["evaluate", "scroll", "resize", "close_tab"],
          hostKind: "desktop app",
        },
      }).capabilities,
    ).toMatchObject({
      [CLIENT_CAPS.browserHost]: {
        supportedCommands: ["evaluate", "scroll", "resize", "close_tab"],
        hostKind: "desktop app",
      },
    });
  });

  test("browser capabilities remain valid when no browser host is advertised", () => {
    expect(parseBrowserCapabilities({}).capabilities).toBeUndefined();
  });

  test("daemon to browser host execute request is an outbound session message", () => {
    const parsed = SessionOutboundMessageSchema.parse({
      type: "browser.automation.execute.request",
      requestId: "req-1",
      command: { command: "snapshot", args: { browserId } },
    });

    expect(parsed.type).toBe("browser.automation.execute.request");
  });

  test("browser host to daemon execute response is an inbound session message", () => {
    const parsed = SessionInboundMessageSchema.parse({
      type: "browser.automation.execute.response",
      payload: {
        requestId: "req-1",
        ok: true,
        result: { command: "list_tabs", tabs: [] },
      },
    });

    expect(parsed.type).toBe("browser.automation.execute.response");
  });

  test("mutable daemon config defaults browser tools off and accepts opt-in patches", () => {
    expect(
      MutableDaemonConfigSchema.parse({
        mcp: { injectIntoAgents: false },
      }).browserTools,
    ).toEqual({ enabled: false });

    expect(
      MutableDaemonConfigPatchSchema.parse({
        browserTools: { enabled: true },
      }).browserTools,
    ).toEqual({ enabled: true });
  });
});
