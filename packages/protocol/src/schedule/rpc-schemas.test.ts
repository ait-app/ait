import { describe, expect, it } from "vitest";
import {
  ScheduleCreateRequestSchema,
  ScheduleListRequestSchema,
  ScheduleUpdateRequestSchema,
} from "./rpc-schemas.js";

describe("schedule RPC schemas", () => {
  it("round-trips new-agent run options on create requests", () => {
    expect(
      ScheduleCreateRequestSchema.parse({
        type: "schedule.create.request",
        workspaceId: "wks-example",
        requestId: "request-1",
        prompt: "Run the task",
        cadence: { type: "every", everyMs: 60_000 },
        target: {
          type: "new-agent",
          config: {
            provider: "claude",
            cwd: "/tmp/project",
            thinkingOptionId: "think-hard",
            archiveOnFinish: false,
            isolation: "worktree",
          },
        },
      }),
    ).toEqual({
      type: "schedule.create.request",
      workspaceId: "wks-example",
      requestId: "request-1",
      prompt: "Run the task",
      cadence: { type: "every", everyMs: 60_000 },
      target: {
        type: "new-agent",
        config: {
          provider: "claude",
          cwd: "/tmp/project",
          thinkingOptionId: "think-hard",
          archiveOnFinish: false,
          isolation: "worktree",
        },
      },
    });
  });

  it("round-trips new-agent run options on update requests", () => {
    expect(
      ScheduleUpdateRequestSchema.parse({
        type: "schedule.update.request",
        requestId: "request-1",
        scheduleId: "schedule-1",
        newAgentConfig: {
          thinkingOptionId: "think-hard",
          archiveOnFinish: false,
          isolation: "worktree",
        },
      }),
    ).toEqual({
      type: "schedule.update.request",
      requestId: "request-1",
      scheduleId: "schedule-1",
      newAgentConfig: {
        thinkingOptionId: "think-hard",
        archiveOnFinish: false,
        isolation: "worktree",
      },
    });
  });
});

it("requires workspace ownership and accepts once targets without a client cwd", () => {
  const request = {
    type: "schedule.create.request",
    requestId: "create-1",
    prompt: "Review",
    cadence: { type: "once", at: "2030-01-01T09:00:00+08:00" },
    target: { type: "new-agent", config: { provider: "codex" } },
  };
  expect(ScheduleCreateRequestSchema.safeParse(request).success).toBe(false);
  expect(ScheduleCreateRequestSchema.safeParse({ ...request, workspaceId: " " }).success).toBe(
    false,
  );
  expect(ScheduleCreateRequestSchema.parse({ ...request, workspaceId: "wks-1" })).toEqual({
    ...request,
    workspaceId: "wks-1",
  });
});

it("preserves scoped lists, full target updates and nullable trigger times", () => {
  const list = { type: "schedule.list.request", requestId: "list-1", workspaceId: "wks-1" };
  expect(ScheduleListRequestSchema.parse(list)).toEqual(list);
  const update = {
    type: "schedule.update.request",
    requestId: "update-1",
    scheduleId: "schedule-1",
    workspaceId: "wks-1",
    target: { type: "agent", agentId: "11111111-1111-4111-8111-111111111111" },
    startAt: null,
    expiresAt: null,
    maxRuns: 2,
  };
  expect(ScheduleUpdateRequestSchema.parse(update)).toEqual(update);
});
