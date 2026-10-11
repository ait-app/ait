import { describe, expect, it } from "vitest";
import {
  KnownStatusPayloadSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
  WSInboundMessageSchema,
  WSOutboundMessageSchema,
} from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

const retiredRequests = [
  "hub.execution.agent.create.request",
  "hub.execution.agent.validate.request",
  "hub.execution.control.request",
  "hub.management.daemon.connect.request",
  "hub.management.daemon.get_status.request",
  "hub.management.daemon.disconnect.request",
  "hub.management.daemon.permissions.update.request",
  "plugin.catalog.get.request",
  "plugin.list.request",
  "plugin.logs.get.request",
  "plugin.directory.install.request",
  "plugin.directory.inspect.request",
  "plugin.source.install.request",
  "plugin.source.status.request",
  "plugin.source.update.request",
  "plugin.source.update.preview.request",
  "plugin.source.update.apply.request",
  "plugin.reload.request",
  "plugin.enable.request",
  "plugin.disable.request",
  "plugin.remove.request",
  "plugin.rpc.invoke.request",
  "chat/create",
  "chat/list",
  "chat/inspect",
  "chat/delete",
  "chat/post",
  "chat/read",
  "chat/wait",
  "loop/run",
  "loop/list",
  "loop/inspect",
  "loop/logs",
  "loop/stop",
];

const retiredEvents = ["hub.execution.agent.update", "hub.execution.agent.stream"];

describe("retired feature messages", () => {
  it.each(retiredRequests)("rejects %s as an unknown request type", (type) => {
    const message = { type, requestId: "retired-request" };
    // A removed discriminator must fail before validating its former payload fields.
    expect(SessionInboundMessageSchema.safeParse(message)).toMatchObject({
      success: false,
      error: { issues: [{ code: "invalid_union", note: "No matching discriminator" }] },
    });
    expect(WSInboundMessageSchema.safeParse({ type: "session", message }).success).toBe(false);
  });

  it.each([
    ...retiredRequests.map((type) =>
      type.endsWith(".request") ? type.replace(/\.request$/, ".response") : `${type}/response`,
    ),
    ...retiredEvents,
  ])("rejects %s in both source and compiled outbound validation", (type) => {
    const message = { type, payload: { requestId: "retired-request" } };
    const envelope = { type: "session", message };
    expect(SessionOutboundMessageSchema.safeParse(message)).toMatchObject({
      success: false,
      error: { issues: [{ code: "invalid_union", note: "No matching discriminator" }] },
    });
    expect(WSOutboundMessageSchema.safeParse(envelope).success).toBe(false);
    expect(validateWSOutboundMessage(envelope).success).toBe(false);
  });

  it.each([...retiredEvents, "status.plugin_catalog_changed", "status.plugin_settings_changed"])(
    "rejects the retired subscription category %s",
    (event) => {
      expect(
        SessionInboundMessageSchema.safeParse({
          type: "session.events.set_subscription.request",
          requestId: "subscription",
          events: ["project.update", event],
        }).success,
      ).toBe(false);
    },
  );

  it.each(["plugin_catalog_changed", "plugin_settings_changed"])(
    "removes %s from known status payloads",
    (status) => {
      expect(
        KnownStatusPayloadSchema.safeParse({ status, pluginId: "example", settingsId: "settings" })
          .success,
      ).toBe(false);
    },
  );

  it("still accepts current event subscriptions and their acknowledgement", () => {
    const request = {
      type: "session.events.set_subscription.request",
      requestId: "subscription",
      events: ["project.update", "provider.snapshot.update", "status.daemon_config_changed"],
      notifications: true,
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    const envelope = {
      type: "session",
      message: {
        type: "session.events.set_subscription.response",
        payload: { requestId: "subscription", subscriptionId: "subscription-1" },
      },
    };
    expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
  });
});
