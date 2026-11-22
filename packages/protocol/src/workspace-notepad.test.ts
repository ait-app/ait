import { expect, it } from "vitest";
import { SessionInboundMessageSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";
import { WORKSPACE_NOTEPAD_METHODS, WorkspaceNotepadSchema } from "./workspace-notepad.js";

function outbound(type: string, payload: Record<string, unknown>) {
  const message = { type: "session", message: { type, payload: { ...payload, requestId: "r1" } } };
  // The app validates with the generated validator; both must agree.
  expect(validateWSOutboundMessage(message).success).toBe(
    WSOutboundMessageSchema.safeParse(message).success,
  );
  return WSOutboundMessageSchema.safeParse(message);
}

it("declares the two notepad methods", () => {
  expect(WORKSPACE_NOTEPAD_METHODS).toEqual([
    "workspace.notepad.get.request",
    "workspace.notepad.write.request",
  ]);
});

it("validates notepad requests", () => {
  const get = { type: "workspace.notepad.get.request", requestId: "r1", workspaceId: "wks_1" };
  expect(SessionInboundMessageSchema.safeParse(get).success).toBe(true);
  expect(SessionInboundMessageSchema.safeParse({ ...get, workspaceId: "" }).success).toBe(false);

  const write = {
    type: "workspace.notepad.write.request",
    requestId: "r1",
    workspaceId: "wks_1",
    text: "",
    expected: 0,
  };
  // Replacing with empty text clears the notepad.
  expect(SessionInboundMessageSchema.safeParse(write).success).toBe(true);
  expect(SessionInboundMessageSchema.safeParse({ ...write, mode: "append" }).success).toBe(true);
  expect(SessionInboundMessageSchema.safeParse({ ...write, mode: "prepend" }).success).toBe(false);
  expect(SessionInboundMessageSchema.safeParse({ ...write, expected: -1 }).success).toBe(false);
  expect(SessionInboundMessageSchema.safeParse({ ...write, text: undefined }).success).toBe(false);
});

it("reads a never-written notepad and one with an author", () => {
  const empty = { workspaceId: "wks_1", revision: 0, text: "" };
  expect(outbound("workspace.notepad.get.response", empty).success).toBe(true);
  expect(WorkspaceNotepadSchema.parse(empty).updatedBy).toBeUndefined();
  expect(
    outbound("workspace.notepad.get.response", {
      workspaceId: "wks_1",
      revision: 3,
      text: "Use v2 of the API.",
      updatedBy: { kind: "agent", agentId: "agt_1", title: "Reviewer" },
      updatedAt: "2026-11-22T09:00:00Z",
    }).success,
  ).toBe(true);
  expect(outbound("workspace.notepad.get.response", { ...empty, text: undefined }).success).toBe(
    false,
  );
});

it("keeps a notepad readable when a later host adds an author kind", () => {
  expect(
    WorkspaceNotepadSchema.parse({
      workspaceId: "wks_1",
      revision: 1,
      text: "x",
      updatedBy: { kind: "channel" },
    }).updatedBy?.kind,
  ).toBe("channel");
});

it("validates accepted and refused writes", () => {
  expect(
    outbound("workspace.notepad.write.response", {
      accepted: true,
      workspaceId: "wks_1",
      revision: 4,
      updatedBy: { kind: "user" },
      updatedAt: "2026-11-22T09:00:00Z",
    }).success,
  ).toBe(true);
  expect(
    outbound("workspace.notepad.write.response", {
      accepted: false,
      workspaceId: "wks_1",
      revision: 5,
      updatedBy: { kind: "bot" },
      error: "The notepad changed. Read it again before replacing it.",
    }).success,
  ).toBe(true);
  expect(
    outbound("workspace.notepad.write.response", { workspaceId: "wks_1", revision: 5 }).success,
  ).toBe(false);
});
