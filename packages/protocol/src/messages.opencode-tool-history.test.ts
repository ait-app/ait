import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { AgentTimelineItemPayloadSchema } from "./messages.js";

const cases = JSON.parse(
  readFileSync(
    new URL(
      "../../../crates/provider/opencode/tests/fixtures/opencode-tool-history.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as Array<{ name: string; expected: unknown }>;

// Rust projects each native message into this exact shared expected item.
for (const fixture of cases) {
  test(`OpenCode tool history satisfies the frontend schema: ${fixture.name}`, () => {
    expect(AgentTimelineItemPayloadSchema.parse(fixture.expected)).toEqual(fixture.expected);
  });
}
