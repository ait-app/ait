import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { AgentPermissionResponse } from "@ait/protocol/agent-types";

const QUESTION_RESPONSE_TIMEOUT_MS = 5_000;

/** Bound the whole operation, including a stalled send or response cleanup. */
export async function respondToQuestionWithTimeout(
  client: Pick<DaemonClient, "respondToPermissionAndWait">,
  input: { agentId: string; requestId: string; response: AgentPermissionResponse },
) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error("Timed out waiting for question response")),
      QUESTION_RESPONSE_TIMEOUT_MS,
    );
  });
  try {
    return await Promise.race([
      client.respondToPermissionAndWait(
        input.agentId,
        input.requestId,
        input.response,
        QUESTION_RESPONSE_TIMEOUT_MS,
      ),
      deadline,
    ]);
  } finally {
    clearTimeout(timer);
  }
}
