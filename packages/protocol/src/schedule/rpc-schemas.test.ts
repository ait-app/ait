import { describe, expect, it } from "vitest";
import {
  ScheduleCreateRequestSchema,
  ScheduleListRequestSchema,
  ScheduleUpdateRequestSchema,
} from "./rpc-schemas.js";
import { ScheduleTargetSchema } from "./types.js";

describe("schedule RPC schemas", () => {
  it.each(["codex/gpt-5", "codex/namespace/model", "", " "])(
    "rejects invalid provider %j in create, target replacement and config patches",
    (provider) => {
      const target = { type: "new-agent", config: { provider } };
      const create = ScheduleCreateRequestSchema.safeParse({
        type: "schedule.create.request",
        requestId: "create-1",
        workspaceId: "wks-example",
        prompt: "Run the task",
        cadence: { type: "every", everyMs: 60_000 },
        target,
      });
      const update = { type: "schedule.update.request", requestId: "update-1", scheduleId: "s" };
      for (const result of [
        create,
        ScheduleUpdateRequestSchema.safeParse({ ...update, target }),
        ScheduleUpdateRequestSchema.safeParse({ ...update, newAgentConfig: { provider } }),
      ]) {
        expect(result.success).toBe(false);
        if (provider.includes("/")) {
          expect(result.error?.issues[0]?.message).toContain("bare provider id");
          expect(result.error?.issues[0]?.message).toContain("separate model field");
        }
      }
    },
  );

  it("trims bare provider ids and preserves model namespaces in all write forms", () => {
    const config = { provider: " custom-provider ", model: "namespace/model" };
    const target = { type: "new-agent", config };
    const create = ScheduleCreateRequestSchema.parse({
      type: "schedule.create.request",
      requestId: "create-1",
      workspaceId: "wks-example",
      prompt: "Run the task",
      cadence: { type: "every", everyMs: 60_000 },
      target,
    });
    const update = ScheduleUpdateRequestSchema.parse({
      type: "schedule.update.request",
      requestId: "update-1",
      scheduleId: "s",
      target,
      newAgentConfig: config,
    });
    const expected = { ...target, config: { ...config, provider: "custom-provider" } };
    expect(create.target).toEqual(expected);
    expect(update.target).toEqual(expected);
    expect(update.newAgentConfig).toEqual(expected.config);
  });

  it("still reads legacy provider/model targets and accepts unrelated patches", () => {
    const target = { type: "new-agent", config: { provider: "codex/gpt-5" } };
    expect(ScheduleTargetSchema.parse(target)).toEqual(target);
    expect(
      ScheduleUpdateRequestSchema.safeParse({
        type: "schedule.update.request",
        requestId: "update-1",
        scheduleId: "s",
        prompt: "Updated task",
      }).success,
    ).toBe(true);
  });

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
