import type { DaemonClient } from "@ait/client/internal/daemon-client";

const setters: Record<string, string> = {
  autoCompaction: "set_auto_compaction",
  autoRetry: "set_auto_retry",
  fastMode: "set_fast_mode",
  slowMode: "set_slow_mode",
};

/** Native runtime toggles commit their acknowledged value; other features keep config semantics. */
export async function applyRuntimeFeature(
  client: Pick<DaemonClient, "runNativeAction" | "setAgentFeature">,
  agentId: string,
  provider: string,
  featureId: string,
  value: unknown,
) {
  const setter = provider === "omp" ? setters[featureId] : undefined;
  if (setter && typeof value === "boolean") {
    await client.runNativeAction(agentId, setter, { enabled: String(value) });
    return;
  }
  await client.setAgentFeature(agentId, featureId, value);
}
