import { describe, expect, it } from "vitest";
import type { AgentStreamEventPayload } from "@ait/protocol/messages";
import { applyStreamEvent, createUserMessage, hydrateStreamState } from "./stream";

function imageEvent(text = "这是什么植物？"): AgentStreamEventPayload {
  return {
    type: "timeline",
    provider: "codex",
    turnId: "turn",
    item: {
      type: "user_message",
      messageId: "native-message",
      clientMessageId: "mobile-message",
      text,
      imageSources: ["/host/images/plant.png"],
    },
  };
}

describe("user images received on another device", () => {
  it.each(["这是什么植物？", ""])("keeps live image messages with text %j", (text) => {
    const result = applyStreamEvent({
      tail: [],
      head: [],
      event: imageEvent(text),
      timestamp: new Date(1),
    });
    expect(result.tail).toEqual([
      expect.objectContaining({ text, imageSources: ["/host/images/plant.png"] }),
    ]);
  });

  it("restores image previews from history without the sending device's cache", () => {
    const result = hydrateStreamState([{ event: imageEvent(""), timestamp: new Date(1) }]);
    expect(result).toEqual([
      expect.objectContaining({ text: "", imageSources: ["/host/images/plant.png"] }),
    ]);
  });

  it("keeps the original attachment preview on the sending device", () => {
    const local = createUserMessage({
      clientMessageId: "mobile-message",
      text: "这是什么植物？",
      timestamp: new Date(1),
      images: [
        {
          id: "original",
          storageType: "native-file",
          storageKey: "/phone/plant.jpg",
          mimeType: "image/jpeg",
          createdAt: 1,
        },
      ],
    });
    const result = applyStreamEvent({
      tail: [local],
      head: [],
      event: imageEvent(),
      timestamp: new Date(2),
    });
    expect(result.tail).toHaveLength(1);
    expect(result.tail[0]).toMatchObject({ images: local.images, text: local.text });
  });

  it("updates a cached placeholder when refreshed native history supplies image sources", () => {
    const cached = createUserMessage({
      messageId: "native-message",
      text: "这是什么植物？\n[Image attachment]",
      timestamp: new Date(1),
    });
    const result = applyStreamEvent({
      tail: [cached],
      head: [],
      event: imageEvent(),
      timestamp: new Date(2),
    });
    expect(result.tail).toEqual([
      expect.objectContaining({ text: "这是什么植物？", imageSources: ["/host/images/plant.png"] }),
    ]);
  });
});
