import { expect, it } from "vitest";
import { acceptAgentDirectoryUpdate } from "./agent-directory-update-policy";

it("keeps a confirmed outcome when an older observation returns with the same native timestamp", () => {
  const current = {
    updatedAt: "2026-10-31T00:00:00Z",
    status: "idle",
    observation: { revision: 3 },
  };
  const delayed = { ...current, status: "running", observation: { revision: 2 } };
  expect(acceptAgentDirectoryUpdate(current, delayed)).toBe(current);
});

it("accepts a newer observation revision independently of native clock precision", () => {
  const cached = {
    updatedAt: "2026-10-31T00:00:01Z",
    status: "running",
    observation: { revision: 2 },
  };
  const confirmed = {
    updatedAt: "2026-10-31T00:00:00Z",
    status: "idle",
    observation: { revision: 3 },
  };
  expect(acceptAgentDirectoryUpdate(cached, confirmed)).toBe(confirmed);
  const ordinary = { updatedAt: "2026-10-31T00:00:01Z", status: "idle" };
  expect(
    acceptAgentDirectoryUpdate(ordinary, { ...ordinary, updatedAt: "2026-10-31T00:00:00Z" }),
  ).toBe(ordinary);
});
