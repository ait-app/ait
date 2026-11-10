/**
 * @vitest-environment jsdom
 */
import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ImageAttachment } from "@/composer/types";

const clipboard = vi.hoisted(() => ({
  collect: vi.fn(),
  save: vi.fn(),
}));

vi.mock("@/constants/platform", () => ({ isWeb: true }));
vi.mock("@/utils/image-attachments-from-files", () => ({
  collectImageFilesFromClipboardData: clipboard.collect,
  filesToImageAttachments: clipboard.save,
}));

import { usePasteImagesEffect } from "./use-paste-images-effect";

const image = { id: "img-1" } as ImageAttachment;

function createDeferredSave() {
  let resolve!: (images: ImageAttachment[]) => void;
  clipboard.save.mockReturnValue(
    new Promise<ImageAttachment[]>((done) => {
      resolve = done;
    }),
  );
  return { resolve: (images: ImageAttachment[]) => resolve(images) };
}

function paste(textarea: HTMLElement) {
  const event = new Event("paste", { cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: {} });
  textarea.dispatchEvent(event);
}

function renderPasteEffect(overrides: { trackImagePasteSave?: (save: Promise<unknown>) => void }) {
  const textarea = document.createElement("textarea");
  const onAddImages = vi.fn();
  const baseProps = {
    textarea,
    isConnected: true,
    disabled: false,
    isDictating: false,
    isRealtimeVoiceForCurrentAgent: false,
    onAddImages,
    trackImagePasteSave: overrides.trackImagePasteSave,
  };
  const view = renderHook((props: typeof baseProps) => usePasteImagesEffect(props), {
    initialProps: baseProps,
  });
  return { textarea, onAddImages, baseProps, view };
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("usePasteImagesEffect", () => {
  beforeEach(() => {
    clipboard.collect.mockReset();
    clipboard.save.mockReset();
    clipboard.collect.mockReturnValue([new File(["x"], "paste.png", { type: "image/png" })]);
  });

  it("still adds the image when the input is disabled during the save", async () => {
    const save = createDeferredSave();
    const { textarea, onAddImages, baseProps, view } = renderPasteEffect({});

    paste(textarea);
    view.rerender({ ...baseProps, disabled: true });
    save.resolve([image]);
    await flush();

    expect(onAddImages).toHaveBeenCalledExactlyOnceWith([image]);
  });

  it("tracks the save until the image has been added", async () => {
    const save = createDeferredSave();
    const trackImagePasteSave = vi.fn();
    const { textarea, onAddImages } = renderPasteEffect({ trackImagePasteSave });

    paste(textarea);
    expect(trackImagePasteSave).toHaveBeenCalledOnce();
    const tracked = trackImagePasteSave.mock.calls[0]?.[0] as Promise<unknown>;

    let settled = false;
    void tracked.then(() => {
      settled = true;
      return undefined;
    });
    await flush();
    expect(settled).toBe(false);

    save.resolve([image]);
    await tracked;
    expect(onAddImages).toHaveBeenCalledOnce();
  });

  it("drops the image when the input unmounts during the save", async () => {
    const save = createDeferredSave();
    const { textarea, onAddImages, view } = renderPasteEffect({});

    paste(textarea);
    view.unmount();
    save.resolve([image]);
    await flush();

    expect(onAddImages).not.toHaveBeenCalled();
  });
});
