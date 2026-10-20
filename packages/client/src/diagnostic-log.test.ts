import { expect, test } from "vitest";
import { z } from "zod";
import { describeValidationIssues, formatDiagnosticFields } from "./diagnostic-log";

test("retains RPC identities and non-enumerable Error details in desktop log text", () => {
  const error = Object.assign(new Error("Capability was not negotiated"), {
    code: "unsupported_capability",
    requestType: "session.events.set_subscription.request",
    requestId: "request-1",
  });
  const fields = JSON.parse(formatDiagnosticFields({ err: error }));

  expect(fields.err).toMatchObject({
    name: "Error",
    message: "Capability was not negotiated",
    code: "unsupported_capability",
    requestType: "session.events.set_subscription.request",
    requestId: "request-1",
    stack: expect.stringContaining("Capability was not negotiated"),
  });
});

test("redacts authentication fields and credentials in nested error strings", () => {
  const result = formatDiagnosticFields({
    token: "private-token",
    cookie: "private-cookie",
    ticket: "private-ticket",
    context: { apiKey: "private-key", password: "private-password" },
    err: new Error(
      "Bearer private-auth https://user:pass@example.com/?token=private-query api_key=private-inline",
    ),
  });

  for (const secret of [
    "private-token",
    "private-cookie",
    "private-ticket",
    "private-key",
    "private-password",
    "private-auth",
    "user:pass",
    "private-query",
    "private-inline",
  ])
    expect(result).not.toContain(secret);
  expect(result).toContain("[redacted]");
});

test("cycles and oversized diagnostic fields cannot break message delivery", () => {
  const fields: { self?: object; requestId: string } = { requestId: "request-1" };
  fields.self = fields;
  expect(JSON.parse(formatDiagnosticFields(fields))).toEqual({
    self: "[Circular]",
    requestId: "request-1",
  });
  const result = formatDiagnosticFields({ detail: "x".repeat(20_000) });
  expect(result.length).toBeLessThan(8300);
  expect(result).toContain("[truncated]");
});

test("nested union diagnostics describe the failing path without rejected user data", () => {
  const parsed = z
    .object({ value: z.union([z.object({ count: z.number() }), z.enum(["ready"])]) })
    .safeParse({ value: { count: "private-content" } });
  expect(parsed.success).toBe(false);
  if (parsed.success) throw new Error("fixture unexpectedly valid");
  const issues = describeValidationIssues(parsed.error);
  expect(issues).toContainEqual({
    code: "invalid_type",
    path: ["value", "count"],
    expected: "number",
  });
  expect(issues).toContainEqual({ code: "invalid_value", path: ["value"], values: ["ready"] });
  expect(JSON.stringify(issues)).not.toContain("private-content");
});

test("validation diagnostics bound the number of retained issue paths", () => {
  const parsed = z.array(z.number()).safeParse(Array(500).fill("private-content"));
  if (parsed.success) throw new Error("fixture unexpectedly valid");
  expect(describeValidationIssues(parsed.error)).toHaveLength(16);
});

test("preserves non-enumerable causes and aggregate errors with redaction and cycle protection", () => {
  const cause = new Error("Disk full token=private-token");
  const failure = new AggregateError([cause], "Save failed", { cause });
  Object.defineProperty(cause, "cause", { value: failure });
  const text = formatDiagnosticFields({ err: failure, requestId: "rpc-1" });
  const fields = JSON.parse(text);
  expect(fields.err.cause.message).toBe("Disk full token=[redacted]");
  expect(fields.err.cause.cause).toBe("[Circular]");
  expect(fields.err.errors).toEqual(["[Circular]"]);
  expect(text).not.toContain("private-token");
});
