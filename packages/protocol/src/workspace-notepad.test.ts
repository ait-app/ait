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

const notepad = {
  workspaceId: "wks_1",
  path: "/home/me/.ait/notepads/wks_1/notepad.md",
  directory: "/home/me/.ait/notepads/wks_1",
  revision: "9f86d081884c7d65",
  text: "",
  updatedAt: "2026-11-22T09:00:00.000Z",
};

it("validates notepad requests", () => {
  const get = { type: "workspace.notepad.get.request", requestId: "r1", workspaceId: "wks_1" };
  expect(SessionInboundMessageSchema.safeParse(get).success).toBe(true);
  expect(SessionInboundMessageSchema.safeParse({ ...get, workspaceId: "" }).success).toBe(false);

  const write = {
    type: "workspace.notepad.write.request",
    requestId: "r1",
    workspaceId: "wks_1",
    text: "",
    expected: notepad.revision,
  };
  // Replacing with empty text clears the notepad.
  expect(SessionInboundMessageSchema.safeParse(write).success).toBe(true);
  expect(SessionInboundMessageSchema.safeParse({ ...write, mode: "append" }).success).toBe(true);
  expect(SessionInboundMessageSchema.safeParse({ ...write, expected: undefined }).success).toBe(
    true,
  );
  expect(SessionInboundMessageSchema.safeParse({ ...write, mode: "prepend" }).success).toBe(false);
  // A revision is the host's token, not a counter.
  expect(SessionInboundMessageSchema.safeParse({ ...write, expected: 0 }).success).toBe(false);
  expect(SessionInboundMessageSchema.safeParse({ ...write, text: undefined }).success).toBe(false);
});

it("reads a notepad with the file a client opens", () => {
  expect(outbound("workspace.notepad.get.response", notepad).success).toBe(true);
  expect(WorkspaceNotepadSchema.parse({ ...notepad, updatedAt: undefined }).directory).toBe(
    notepad.directory,
  );
  for (const missing of ["path", "directory", "text", "revision"]) {
    expect(
      outbound("workspace.notepad.get.response", { ...notepad, [missing]: undefined }).success,
    ).toBe(false);
  }
  // A host that kept the notepad in its own store had a counter and no file.
  expect(
    outbound("workspace.notepad.get.response", { workspaceId: "wks_1", revision: 0, text: "" })
      .success,
  ).toBe(false);
});

it("validates accepted and refused writes", () => {
  const written = {
    workspaceId: "wks_1",
    path: notepad.path,
    revision: notepad.revision,
    updatedAt: notepad.updatedAt,
  };
  expect(outbound("workspace.notepad.write.response", { ...written, accepted: true }).success).toBe(
    true,
  );
  expect(
    outbound("workspace.notepad.write.response", {
      ...written,
      accepted: false,
      error: "The notepad changed. Read it again before replacing it.",
    }).success,
  ).toBe(true);
  expect(outbound("workspace.notepad.write.response", written).success).toBe(false);
});
