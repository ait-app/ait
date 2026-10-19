import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_CONTENT_MAX_WIDTH } from "@/styles/theme";
import {
  clearAssistantMessageHeightEstimateCache,
  estimateAssistantMessageHeightFromCache,
  setAssistantMessageHeight,
} from "./assistant-message-height-estimate";
describe("assistant message height estimate", () => {
  beforeEach(() => {
    clearAssistantMessageHeightEstimateCache();
  });

  it("uses a whole-message measurement only at the measured width", () => {
    setAssistantMessageHeight({
      markdown: "First\n\nSecond",
      width: DEFAULT_CONTENT_MAX_WIDTH,
      height: 97,
    });
    expect(
      estimateAssistantMessageHeightFromCache({
        markdown: "First\n\nSecond",
        contentMaxWidth: DEFAULT_CONTENT_MAX_WIDTH,
      }),
    ).toBe(97);
    expect(
      estimateAssistantMessageHeightFromCache({
        markdown: "First\n\nSecond",
        contentMaxWidth: 1600,
      }),
    ).toBeNull();
  });
});

it("scopes measured Markdown images to their host and workspace", () => {
  const markdown = "![local](image.png)";
  const imageContext = { serverId: "one", workspaceRoot: "/repo" };
  setAssistantMessageHeight({ markdown, width: 600, height: 900, imageContext });
  expect(
    estimateAssistantMessageHeightFromCache({ markdown, contentMaxWidth: 600, imageContext }),
  ).toBe(900);
  expect(
    estimateAssistantMessageHeightFromCache({
      markdown,
      contentMaxWidth: 600,
      imageContext: { ...imageContext, serverId: "two" },
    }),
  ).not.toBe(900);
});
