/** @vitest-environment jsdom */
import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMarkdownSource } from "./images";

const mocks = vi.hoisted(() => ({
  acquireRetained: vi.fn(),
  release: vi.fn(),
}));

vi.mock("react-native", () => ({ Image: { getSize: vi.fn() } }));
vi.mock("@/constants/platform", () => ({ isWeb: true }));
vi.mock("@/data/query", () => ({
  // A fresh array (and fresh result objects) on every render, like useQueries.
  useFetchQueries: (inputs: { queryKey: unknown[] }[]) =>
    inputs.map((input) => ({
      data: input.queryKey[0] === "markdownImage" ? { uri: "blob:a", attachment: {} } : undefined,
      isError: false,
    })),
}));
vi.mock("@/assistant-image/file-acquisition", () => ({
  createAssistantImageFileAcquisition: () => ({ key: "file:a", locate: vi.fn() }),
}));
vi.mock("@/assistant-image/acquisition-cache", () => ({
  createAssistantImageAcquisitionCache: () => ({
    acquire: vi.fn(),
    acquireRetained: mocks.acquireRetained,
  }),
}));
vi.mock("@/attachments/service", () => ({}));
vi.mock("@/attachments/gc-retention", () => ({}));
vi.mock("@/utils/assistant-image-source", () => ({ resolveAssistantImageSource: () => ({}) }));
vi.mock("./image-source", () => ({ resolveMarkdownPreviewImageSource: () => "a.png" }));
vi.mock("./image-decoding", () => ({
  decodeMarkdownImage: vi.fn(),
  getMarkdownImageStatus: () => "ready",
}));
vi.mock("./source", () => ({
  prepareMarkdownSource: () => ({
    images: [{ source: "a.png" }],
    render: () => "",
    linkRecognition: null,
    linkSource: "",
  }),
}));

describe("useMarkdownSource", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.acquireRetained.mockReturnValue({ promise: Promise.resolve(), release: mocks.release });
  });

  it("keeps a resolved image retained across re-renders", () => {
    const context = { client: null, workspaceRoot: "/repo", filePath: "README.md" };
    const { rerender, unmount } = renderHook(() => useMarkdownSource("![a](a.png)", context));
    rerender();
    rerender();

    expect(mocks.acquireRetained).toHaveBeenCalledOnce();
    expect(mocks.acquireRetained).toHaveBeenCalledWith("file:a", expect.any(Function));
    expect(mocks.release).not.toHaveBeenCalled();

    unmount();
    expect(mocks.release).toHaveBeenCalledOnce();
  });
});
