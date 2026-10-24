import { useCallback, useRef, useState } from "react";
import type { useVoiceOptional } from "@/contexts/voice-context";

/** Creates a draft's empty session before handing its real ID to the voice runtime. */
export function useComposerVoiceStart({
  voice,
  serverId,
  agentId,
  hasAgent,
  supportsRealtimeVoice,
  disabled,
  createVoiceAgent,
  onError,
}: {
  voice: ReturnType<typeof useVoiceOptional>;
  serverId: string;
  agentId: string;
  hasAgent: boolean;
  supportsRealtimeVoice: boolean;
  disabled: boolean;
  createVoiceAgent?: () => Promise<string | null>;
  onError: (message: string) => void;
}) {
  const isAvailable = supportsRealtimeVoice && (hasAgent || Boolean(createVoiceAgent));
  const pending = useRef(false);
  const [isStarting, setIsStarting] = useState(false);
  const startVoice = useCallback(async () => {
    if (!isAvailable || !voice || disabled || pending.current || voice.isVoiceSwitching) return;
    if (hasAgent && voice.isVoiceModeForAgent(serverId, agentId)) return;
    if (!hasAgent && !createVoiceAgent) return;
    pending.current = true;
    setIsStarting(true);
    try {
      const targetId = hasAgent ? agentId : await createVoiceAgent?.();
      if (targetId) await voice.startVoice(serverId, targetId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.trim()) onError(message);
    } finally {
      pending.current = false;
      setIsStarting(false);
    }
  }, [isAvailable, voice, serverId, agentId, hasAgent, disabled, createVoiceAgent, onError]);
  return { startVoice, isStarting, isAvailable };
}
