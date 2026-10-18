import { beforeEach, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { encodeImages } from "@/utils/encode-images";
import { requestWorkspaceDraftAgent } from "./create-agent-request";

vi.mock("@/utils/encode-images", () => ({ encodeImages: vi.fn() }));

beforeEach(() => vi.clearAllMocks());

it("keeps a failed image preparation from creating a partial initial session", async () => {
  const failure = new Error("This image cannot fit the message budget");
  vi.mocked(encodeImages).mockRejectedValue(failure);
  const createAgent = vi.fn();
  await expect(
    requestWorkspaceDraftAgent({ createAgent } as unknown as DaemonClient, {
      workspaceId: "workspace",
      clientMessageId: "message",
      config: { provider: "codex", cwd: "/repo" },
      text: "Inspect",
    }),
  ).rejects.toBe(failure);
  expect(createAgent).not.toHaveBeenCalled();
});

it("sends a whole image and only user attachments when creating an Agent", async () => {
  vi.mocked(encodeImages).mockResolvedValue([{ data: "complete-image", mimeType: "image/webp" }]);
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
      images: [{ data: "complete-image", mimeType: "image/webp" }],
      attachments: [{ type: "text", mimeType: "text/plain", text: "Context" }],
    }),
  );
});
