import { beforeEach, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { encodeImages } from "@/utils/encode-images";
import { requestWorkspaceDraftAgent } from "./create-agent-request";

vi.mock("@/utils/encode-images", () => ({ encodeImages: vi.fn() }));

beforeEach(() => vi.clearAllMocks());

it("prepares image grouping for a new Agent as well as subsequent messages", async () => {
  vi.mocked(encodeImages).mockResolvedValue([
    { data: "overview", mimeType: "image/png", description: "original image 1: overview" },
    { data: "detail", mimeType: "image/webp", description: "original image 1: detail crop" },
  ]);
  const createAgent = vi.fn(async () => ({ id: "agent" }));
  await requestWorkspaceDraftAgent({ createAgent } as unknown as DaemonClient, {
    workspaceId: "workspace",
    clientMessageId: "message",
    config: { provider: "codex", cwd: "/repo" },
    text: "Read this image",
    attachments: [{ type: "text", mimeType: "text/plain", text: "Context" }],
  });
  expect(encodeImages).toHaveBeenCalledWith(undefined, {
    text: "Read this image",
    clientMessageId: "message",
    attachments: [{ type: "text", mimeType: "text/plain", text: "Context" }],
  });
  expect(createAgent).toHaveBeenCalledWith(
    expect.objectContaining({
      images: [
        { data: "overview", mimeType: "image/png" },
        { data: "detail", mimeType: "image/webp" },
      ],
      attachments: [
        { type: "text", mimeType: "text/plain", text: "Context" },
        expect.objectContaining({ type: "text", text: expect.stringContaining("Image layout") }),
      ],
    }),
  );
});
