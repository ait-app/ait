import { describe, expect, test } from "vitest";

import { buildProviderCommand } from "@/utils/provider-command-templates";

describe("buildProviderCommand", () => {
  test("builds Antigravity resume commands from native conversation ids", () => {
    expect(
      buildProviderCommand({ provider: "antigravity", id: "resume", sessionId: "native-id" }),
    ).toBe("agy --conversation native-id");
  });
  test("builds Cursor resume commands from native session ids", () => {
    expect(buildProviderCommand({ provider: "cursor", id: "resume", sessionId: "native-id" })).toBe(
      "cursor-agent --resume native-id",
    );
  });
  test("builds Hermes resume commands from native session ids", () => {
    expect(
      buildProviderCommand({
        provider: "hermes",
        id: "resume",
        sessionId: "20260813_111500_abc123",
      }),
    ).toBe("hermes --resume 20260813_111500_abc123");
  });

  test("builds OpenCode resume commands from native session ids", () => {
    expect(
      buildProviderCommand({
        provider: "opencode",
        id: "resume",
        sessionId: "ses_abc123",
      }),
    ).toBe("opencode --session ses_abc123");
  });
});
