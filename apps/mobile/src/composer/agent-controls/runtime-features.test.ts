import { expect, it, vi } from "vitest";
import { applyRuntimeFeature } from "./runtime-features";

it("uses native acknowledged runtime setters without writing an unconfirmed config value", async () => {
  const client = {
    runNativeAction: vi.fn().mockResolvedValue({ text: "" }),
    setAgentFeature: vi.fn(),
  };
  await applyRuntimeFeature(client, "agent", "omp", "autoCompaction", false);
  expect(client.runNativeAction).toHaveBeenCalledExactlyOnceWith("agent", "set_auto_compaction", {
    enabled: "false",
  });
  expect(client.setAgentFeature).not.toHaveBeenCalled();
  client.runNativeAction.mockRejectedValueOnce(new Error("Rejected"));
  await expect(applyRuntimeFeature(client, "agent", "omp", "autoRetry", true)).rejects.toThrow(
    "Rejected",
  );
  expect(client.setAgentFeature).not.toHaveBeenCalled();
});
it("retains next-turn configuration for model roles and other providers", async () => {
  const client = {
    runNativeAction: vi.fn(),
    setAgentFeature: vi.fn().mockResolvedValue(undefined),
  };
  await applyRuntimeFeature(client, "agent", "omp", "modelRole.advisor", "native");
  await applyRuntimeFeature(client, "agent", "claude", "autoRetry", true);
  expect(client.runNativeAction).not.toHaveBeenCalled();
  expect(client.setAgentFeature.mock.calls).toEqual([
    ["agent", "modelRole.advisor", "native"],
    ["agent", "autoRetry", true],
  ]);
});
