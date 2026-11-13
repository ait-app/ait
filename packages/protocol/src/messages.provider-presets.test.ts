import { describe, expect, test } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";

describe("provider preset wire messages", () => {
  const preset = {
    name: "Review",
    model: "provider/model",
    thinkingOptionId: "high",
    featureValues: {
      "modelRole.advisor": "native",
      "modelRole.worker": "unset",
      "modelRole.summarizer": JSON.stringify({ model: "other/model", thinkingLevel: "low" }),
    },
  };

  test("save and list preserve explicit inherit, clear and model routing values", () => {
    const request = {
      type: "provider.presets.save.request",
      provider: "omp",
      cwd: "/project",
      original: "Old name",
      preset,
      requestId: "save",
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
    const response = {
      type: "provider.presets.list.response",
      payload: {
        provider: "omp",
        presets: [{ ...preset, source: "global", editable: true }],
        requestId: "list",
      },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
  });

  test("each mutation has a single response discriminator and preserves failure reasons", () => {
    for (const action of ["save", "delete"]) {
      const response = {
        type: `provider.presets.${action}.response`,
        payload: { provider: "omp", error: "Project preset is read-only", requestId: action },
      };
      expect(SessionOutboundMessageSchema.parse(response)).toEqual(response);
    }
    // Consumers which derive legacy unions can inspect each discriminator individually.
    expect(
      SessionOutboundMessageSchema.options.every(
        (option) => typeof option.shape.type.value === "string",
      ),
    ).toBe(true);
  });

  test("native model routes longer than a display name survive the save boundary", () => {
    const model = JSON.stringify(["provider", "model/" + "part/".repeat(64)]);
    const request = {
      type: "provider.presets.save.request",
      provider: "omp",
      preset: { ...preset, model },
      requestId: "long-model-route",
    };
    expect(SessionInboundMessageSchema.parse(request)).toEqual(request);
  });
});
