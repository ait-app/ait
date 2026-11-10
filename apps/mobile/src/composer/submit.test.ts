import { describe, expect, it, vi } from "vitest";
import { markDeliveredSendError } from "@/composer/submission/model";
import { readLiveDraftText, runImmediateClientSlashCommand, submitAgentInput } from "./submit";

function createDeferredPromise<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return {
    promise,
    resolve,
    reject,
  };
}

describe("readLiveDraftText", () => {
  it("prefers the mounted editor over a draft store still awaiting its after-paint clear", () => {
    const stored = { getSnapshot: () => "already sent" };
    expect(readLiveDraftText({ getText: () => "" }, stored)).toBe("");
  });

  it("falls back to the draft store when no editor is mounted", () => {
    expect(readLiveDraftText(null, { getSnapshot: () => "saved" })).toBe("saved");
  });
});

describe("submitAgentInput", () => {
  it.each([
    { isAgentRunning: false, forceSend: false, expected: "submitted", prefix: "" },
    { isAgentRunning: true, forceSend: false, expected: "queued", prefix: "" },
    { isAgentRunning: true, forceSend: true, expected: "submitted", prefix: "" },
    { isAgentRunning: false, forceSend: false, expected: "submitted", prefix: " \t" },
    { isAgentRunning: true, forceSend: false, expected: "queued", prefix: " \t" },
    { isAgentRunning: true, forceSend: true, expected: "submitted", prefix: " \t" },
  ])(
    "preserves command syntax when $expected (forceSend=$forceSend, prefix='$prefix')",
    async (mode) => {
      const message = `${mode.prefix}/goal edit \n  keep "quoted  text"\t \n`;
      const attachments = [{ id: "file-1" }];
      const queueMessage = vi.fn();
      const submitMessage = vi.fn(async () => {});

      await expect(
        submitAgentInput({
          ...mode,
          message,
          attachments,
          canSubmit: true,
          queueMessage,
          submitMessage,
          clearDraft: vi.fn(),
          setUserInput: vi.fn(),
          setAttachments: vi.fn(),
          setSendError: vi.fn(),
          setIsProcessing: vi.fn(),
        }),
      ).resolves.toBe(mode.expected);

      const dispatch = mode.expected === "queued" ? queueMessage : submitMessage;
      const unused = mode.expected === "queued" ? submitMessage : queueMessage;
      expect(dispatch).toHaveBeenCalledExactlyOnceWith({ message, attachments });
      expect(unused).not.toHaveBeenCalled();
    },
  );

  it("restores the exact command and attachments after native rejection", async () => {
    const message = " \t/goal edit \n  objective with trailing whitespace  \n";
    const attachments = [{ id: "file-1" }];
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const clearDraft = vi.fn();
    const submitMessage = vi.fn(async () => {
      throw new Error("Command rejected");
    });

    await expect(
      submitAgentInput({
        message,
        attachments,
        isAgentRunning: false,
        canSubmit: true,
        queueMessage: vi.fn(),
        submitMessage,
        clearDraft,
        setUserInput,
        setAttachments,
        setSendError: vi.fn(),
        setIsProcessing: vi.fn(),
      }),
    ).resolves.toBe("failed");

    expect(submitMessage).toHaveBeenCalledExactlyOnceWith({ message, attachments });
    expect(setUserInput).toHaveBeenNthCalledWith(2, message);
    expect(setAttachments).toHaveBeenNthCalledWith(2, attachments);
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("clears the composer before an in-flight submit resolves", async () => {
    const deferred = createDeferredPromise<void>();
    const queueMessage = vi.fn();
    const submitMessage = vi.fn(async () => {
      await deferred.promise;
    });
    const clearDraft = vi.fn();
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();

    const submitPromise = submitAgentInput({
      message: "  hello world  ",
      attachments: [],
      isAgentRunning: false,
      canSubmit: true,
      queueMessage,
      submitMessage,
      clearDraft,
      setUserInput,
      setAttachments,
      setSendError,
      setIsProcessing,
    });

    expect(queueMessage).not.toHaveBeenCalled();
    expect(submitMessage).toHaveBeenCalledWith({
      message: "hello world",
      attachments: [],
    });
    expect(setUserInput).toHaveBeenCalledWith("");
    expect(setAttachments).toHaveBeenCalledWith([]);
    expect(setSendError).toHaveBeenCalledWith(null);
    expect(setIsProcessing).toHaveBeenCalledWith(true);
    expect(clearDraft).not.toHaveBeenCalled();

    deferred.resolve();

    await expect(submitPromise).resolves.toBe("submitted");
    // The composer was already cleared; clearing again would erase input typed in flight.
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("restores a failed send ahead of input entered while it was in flight", async () => {
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();

    await expect(
      submitAgentInput({
        message: "first",
        attachments: [{ id: "img-1" }],
        isAgentRunning: false,
        canSubmit: true,
        queueMessage: vi.fn(),
        submitMessage: async () => {
          throw new Error("offline");
        },
        clearDraft: vi.fn(),
        readCurrentDraft: () => ({ text: "follow-up", attachments: [{ id: "img-2" }] }),
        setUserInput,
        setAttachments,
        setSendError: vi.fn(),
        setIsProcessing: vi.fn(),
      }),
    ).resolves.toBe("failed");

    expect(setUserInput).toHaveBeenLastCalledWith("first\n\nfollow-up");
    expect(setAttachments).toHaveBeenLastCalledWith([{ id: "img-1" }, { id: "img-2" }]);
  });

  it("keeps the composer clear when the failed message was already delivered", async () => {
    const setUserInput = vi.fn();
    const setSendError = vi.fn();

    await expect(
      submitAgentInput({
        message: "sent once",
        attachments: [],
        isAgentRunning: false,
        canSubmit: true,
        queueMessage: vi.fn(),
        submitMessage: async () => {
          const error = new Error("lost after echo");
          markDeliveredSendError(error);
          throw error;
        },
        clearDraft: vi.fn(),
        setUserInput,
        setAttachments: vi.fn(),
        setSendError,
        setIsProcessing: vi.fn(),
      }),
    ).resolves.toBe("delivered");

    expect(setUserInput).toHaveBeenCalledTimes(1);
    expect(setUserInput).toHaveBeenCalledWith("");
    expect(setSendError).toHaveBeenLastCalledWith("lost after echo");
  });

  it("preserves the composer before an in-flight submit resolves when requested", async () => {
    const deferred = createDeferredPromise<void>();
    const attachments = [{ id: "img-1" }];
    const queueMessage = vi.fn();
    const submitMessage = vi.fn(async () => {
      await deferred.promise;
    });
    const clearDraft = vi.fn();
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();

    const submitPromise = submitAgentInput({
      message: "  keep me  ",
      attachments,
      submitBehavior: "preserve-and-lock",
      isAgentRunning: false,
      canSubmit: true,
      queueMessage,
      submitMessage,
      clearDraft,
      setUserInput,
      setAttachments,
      setSendError,
      setIsProcessing,
    });

    expect(queueMessage).not.toHaveBeenCalled();
    expect(submitMessage).toHaveBeenCalledWith({
      message: "keep me",
      attachments,
    });
    expect(setUserInput).not.toHaveBeenCalled();
    expect(setAttachments).not.toHaveBeenCalled();
    expect(setSendError).toHaveBeenCalledWith(null);
    expect(setIsProcessing).toHaveBeenCalledWith(true);
    expect(clearDraft).not.toHaveBeenCalled();

    deferred.resolve();

    await expect(submitPromise).resolves.toBe("submitted");
    expect(clearDraft).toHaveBeenCalledWith("sent");
  });

  it("queues while the agent is running and clears the composer immediately", async () => {
    const queueMessage = vi.fn();
    const submitMessage = vi.fn();
    const clearDraft = vi.fn();
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();

    await expect(
      submitAgentInput({
        message: "  queued message  ",
        attachments: [{ id: "img-1" }],
        isAgentRunning: true,
        canSubmit: true,
        queueMessage,
        submitMessage,
        clearDraft,
        setUserInput,
        setAttachments,
        setSendError,
        setIsProcessing,
      }),
    ).resolves.toBe("queued");

    expect(queueMessage).toHaveBeenCalledWith({
      message: "queued message",
      attachments: [{ id: "img-1" }],
    });
    expect(submitMessage).not.toHaveBeenCalled();
    expect(setUserInput).toHaveBeenCalledWith("");
    expect(setAttachments).toHaveBeenCalledWith([]);
    expect(setSendError).not.toHaveBeenCalled();
    expect(setIsProcessing).not.toHaveBeenCalled();
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("restores the composer when submit fails", async () => {
    const submitError = new Error("No host selected");
    const queueMessage = vi.fn();
    const submitMessage = vi.fn(async () => {
      throw submitError;
    });
    const clearDraft = vi.fn();
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();
    const onSubmitError = vi.fn();
    const attachments = [{ id: "img-1" }];

    await expect(
      submitAgentInput({
        message: "  hello world  ",
        attachments,
        isAgentRunning: false,
        canSubmit: true,
        queueMessage,
        submitMessage,
        clearDraft,
        setUserInput,
        setAttachments,
        setSendError,
        setIsProcessing,
        onSubmitError,
      }),
    ).resolves.toBe("failed");

    expect(onSubmitError).toHaveBeenCalledWith(submitError);
    expect(setUserInput).toHaveBeenNthCalledWith(1, "");
    expect(setUserInput).toHaveBeenNthCalledWith(2, "hello world");
    expect(setAttachments).toHaveBeenNthCalledWith(1, []);
    expect(setAttachments).toHaveBeenNthCalledWith(2, attachments);
    expect(setSendError).toHaveBeenNthCalledWith(1, null);
    expect(setSendError).toHaveBeenNthCalledWith(2, "No host selected");
    expect(setIsProcessing).toHaveBeenNthCalledWith(1, true);
    expect(setIsProcessing).toHaveBeenNthCalledWith(2, false);
    expect(clearDraft).not.toHaveBeenCalled();
  });

  it("restores a steered active-turn draft after an ambiguous immediate-send error", async () => {
    const error = new Error("connection lost after delivery");
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();

    await expect(
      submitAgentInput({
        message: "  steer this turn  ",
        attachments: [{ id: "img-1" }],
        forceSend: true,
        isAgentRunning: true,
        canSubmit: true,
        queueMessage: vi.fn(),
        submitMessage: async () => {
          throw error;
        },
        clearDraft: vi.fn(),
        setUserInput,
        setAttachments,
        setSendError,
        setIsProcessing,
      }),
    ).resolves.toBe("failed");

    expect(setUserInput).toHaveBeenNthCalledWith(1, "");
    expect(setUserInput).toHaveBeenNthCalledWith(2, "steer this turn");
    expect(setAttachments).toHaveBeenNthCalledWith(1, []);
    expect(setAttachments).toHaveBeenNthCalledWith(2, [{ id: "img-1" }]);
    expect(setSendError).toHaveBeenLastCalledWith("connection lost after delivery");
  });

  it("submits when empty submit is explicitly allowed", async () => {
    const queueMessage = vi.fn();
    const submitMessage = vi.fn(async () => {});
    const clearDraft = vi.fn();
    const setUserInput = vi.fn();
    const setAttachments = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();

    await expect(
      submitAgentInput({
        message: "   ",
        attachments: [],
        allowEmptySubmit: true,
        isAgentRunning: false,
        canSubmit: true,
        queueMessage,
        submitMessage,
        clearDraft,
        setUserInput,
        setAttachments,
        setSendError,
        setIsProcessing,
      }),
    ).resolves.toBe("submitted");

    expect(queueMessage).not.toHaveBeenCalled();
    expect(submitMessage).toHaveBeenCalledWith({
      message: "",
      attachments: [],
    });
    expect(setUserInput).toHaveBeenCalledWith("");
    expect(clearDraft).not.toHaveBeenCalled();
  });
});

describe("runImmediateClientSlashCommand", () => {
  it("keeps the composer until the command succeeds", async () => {
    const deferred = createDeferredPromise<void>();
    const clearComposer = vi.fn();
    const setIsProcessing = vi.fn();

    const result = runImmediateClientSlashCommand({
      run: () => deferred.promise,
      clearComposer,
      setSendError: vi.fn(),
      setIsProcessing,
    });

    expect(setIsProcessing).toHaveBeenCalledWith(true);
    expect(clearComposer).not.toHaveBeenCalled();

    deferred.resolve();

    await expect(result).resolves.toBe("completed");
    expect(clearComposer).toHaveBeenCalledTimes(1);
    expect(setIsProcessing).toHaveBeenLastCalledWith(false);
  });

  it("leaves the command text and attachments in place when the command fails", async () => {
    const clearComposer = vi.fn();
    const setSendError = vi.fn();
    const setIsProcessing = vi.fn();
    const onError = vi.fn();
    const failure = new Error("archive failed");

    await expect(
      runImmediateClientSlashCommand({
        run: async () => {
          throw failure;
        },
        clearComposer,
        setSendError,
        setIsProcessing,
        onError,
      }),
    ).resolves.toBe("failed");

    expect(clearComposer).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(failure);
    expect(setSendError).toHaveBeenLastCalledWith("archive failed");
    expect(setIsProcessing).toHaveBeenLastCalledWith(false);
  });
});
