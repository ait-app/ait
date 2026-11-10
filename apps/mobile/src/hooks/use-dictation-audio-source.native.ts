import { useCallback, useEffect, useRef } from "react";
import { Buffer } from "buffer";
import { useState } from "react";

import { AttemptCancelledError } from "@/utils/attempt-guard";
import { createAudioEngine } from "@/voice/audio-engine";

import type {
  DictationAudioSource,
  DictationAudioSourceConfig,
} from "./use-dictation-audio-source.types";

export function useDictationAudioSource(config: DictationAudioSourceConfig): DictationAudioSource {
  const onPcmSegmentRef = useRef(config.onPcmSegment);
  const onErrorRef = useRef(config.onError);
  const onInterruptionRef = useRef(config.onInterruption);
  const [volume, setVolume] = useState(0);
  const engineRef = useRef<ReturnType<typeof createAudioEngine> | null>(null);
  const interruptionCountRef = useRef(0);

  const getOrCreateEngine = useCallback(() => {
    if (engineRef.current) {
      return engineRef.current;
    }

    engineRef.current = createAudioEngine({
      onCaptureData: (pcm) => {
        onPcmSegmentRef.current(Buffer.from(pcm).toString("base64"));
      },
      onVolumeLevel: (level) => {
        setVolume(level);
      },
      onError: (error) => {
        onErrorRef.current?.(error);
      },
      onInterruption: () => {
        interruptionCountRef.current += 1;
        onInterruptionRef.current?.();
      },
    });
    return engineRef.current;
  }, []);

  useEffect(() => {
    onPcmSegmentRef.current = config.onPcmSegment;
    onErrorRef.current = config.onError;
    onInterruptionRef.current = config.onInterruption;
  }, [config.onPcmSegment, config.onError, config.onInterruption]);

  const start = useCallback(async () => {
    const engine = getOrCreateEngine();
    const interruptionsBeforeStart = interruptionCountRef.current;
    await engine.initialize();
    await engine.startCapture();
    // The engine returns quietly when an interruption cancels the start, so resolving alone
    // does not mean capture began.
    if (interruptionCountRef.current !== interruptionsBeforeStart) {
      throw new AttemptCancelledError();
    }
  }, [getOrCreateEngine]);

  const stop = useCallback(async () => {
    await engineRef.current?.stopCapture();
    setVolume(0);
  }, []);

  useEffect(() => {
    return () => {
      const engine = engineRef.current;
      engineRef.current = null;
      void engine?.destroy().catch(() => undefined);
    };
  }, []);

  return {
    start,
    stop,
    volume,
  };
}
