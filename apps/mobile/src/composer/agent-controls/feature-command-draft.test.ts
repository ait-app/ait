import { expect, it } from "vitest";
import { featureCommandDraft } from "./feature-command-draft";

it("preserves the current draft as command arguments", () => {
  expect(featureCommandDraft("", "/compact ")).toBe("/compact ");
  expect(featureCommandDraft("keep the API details", "/compact ")).toBe(
    "/compact keep the API details",
  );
  expect(featureCommandDraft("/old keep details", "/compact ")).toBe("/compact keep details");
});
