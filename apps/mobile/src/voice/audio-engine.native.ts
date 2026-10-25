import type {
  AudioEngine,
  AudioEngineCallbacks,
  AudioPlaybackSource,
} from "@/voice/audio-engine-types";
import * as native from "@ait/expo-two-way-audio";

interface PendingAudio {
  duration: number;
  resolve: (duration: number) => void;
  reject: (error: Error) => void;
}

interface AudioEngineTraceOptions {
  traceLabel?: string;
}

function parsePcmSampleRate(mimeType: string): number | null {
  const match = /rate=(\d+)/i.exec(mimeType);
  if (!match) {
    return null;
  }
  const rate = Number(match[1]);
  return Number.isFinite(rate) && rate > 0 ? rate : null;
}

function resamplePcm16(pcm: Uint8Array, fromRate: number, toRate: number): Uint8Array {
  if (fromRate === toRate) {
    return pcm;
  }

  const inputSamples = Math.floor(pcm.length / 2);
  const outputSamples = Math.floor((inputSamples * toRate) / fromRate);
  const out = new Uint8Array(outputSamples * 2);
  const ratio = fromRate / toRate;

  const readInt16 = (sampleIndex: number): number => {
    const i = sampleIndex * 2;
    if (i + 1 >= pcm.length) {
      return 0;
    }
    const lo = pcm[i];
    const hi = pcm[i + 1];
    let value = (hi << 8) | lo;
    if (value & 0x8000) {
      value = value - 0x10000;
    }
    return value;
  };

  const writeInt16 = (sampleIndex: number, value: number): void => {
    const clamped = Math.max(-32768, Math.min(32767, Math.round(value)));
    const i = sampleIndex * 2;
    out[i] = clamped & 0xff;
    out[i + 1] = (clamped >> 8) & 0xff;
  };

  for (let i = 0; i < outputSamples; i++) {
    const srcPos = i * ratio;
    const i0 = Math.floor(srcPos);
    const frac = srcPos - i0;
    const s0 = readInt16(i0);
    const s1 = readInt16(Math.min(inputSamples - 1, i0 + 1));
    writeInt16(i, s0 + (s1 - s0) * frac);
  }

  return out;
}

let nextEngineId = 0;

export function createAudioEngine(
  callbacks: AudioEngineCallbacks,
  _options?: AudioEngineTraceOptions,
): AudioEngine {
  const engineId = ++nextEngineId;
  let nextPlaybackId = 0;
  let initialized = false;
  let initializing: Promise<void> | null = null;
  let destroyed = false;
  let captureActive = false;
  let captureGeneration = 0;
  let captureAttempt: number | null = null;
  let muted = false;
  let playbackGeneration = 0;
  let decoding = Promise.resolve();
  const pending = new Map<string, PendingAudio>();

  function releaseSessionIfIdle(): void {
    if (
      initialized &&
      !destroyed &&
      !captureActive &&
      captureAttempt === null &&
      pending.size === 0
    ) {
      native.releaseAudioSession();
    }
  }

  async function ensureInitialized(): Promise<void> {
    if (destroyed) throw new Error("Audio engine destroyed");
    if (initialized) return;
    if (!initializing) {
      const capture = captureGeneration;
      const playback = playbackGeneration;
      initializing = native
        .initialize()
        .then((success: boolean) => {
          if (!success) throw new Error("Audio engine initialization failed");
          if (destroyed) {
            native.tearDown();
            throw new Error("Audio engine destroyed");
          }
          initialized = true;
          // initialize() is also called directly by the runtime, before startCapture().
          // A stop during that wait must release the session even if no media follows.
          if (capture !== captureGeneration || playback !== playbackGeneration) {
            releaseSessionIfIdle();
          }
        })
        .finally(() => {
          initializing = null;
        });
    }
    await initializing;
  }

  function stopPlayback(): void {
    playbackGeneration += 1;
    decoding = Promise.resolve();
    native.stopPlayback();
    for (const item of pending.values()) item.reject(new Error("Playback stopped"));
    pending.clear();
    releaseSessionIfIdle();
  }

  const microphoneSubscription = native.addExpoTwoWayAudioEventListener(
    "onMicrophoneData",
    (event) => {
      if (captureActive && !muted) callbacks.onCaptureData(event.data);
    },
  );
  const volumeSubscription = native.addExpoTwoWayAudioEventListener(
    "onInputVolumeLevelData",
    (event) => {
      if (captureActive) callbacks.onVolumeLevel(muted ? 0 : event.data);
    },
  );
  const interruptionSubscription = native.addExpoTwoWayAudioEventListener(
    "onAudioInterruption",
    (event) => {
      if (event.data !== "blocked" && event.data !== "began") return;
      captureGeneration += 1;
      const wasInUse =
        captureActive || captureAttempt !== null || initializing !== null || pending.size > 0;
      captureAttempt = null;
      captureActive = false;
      muted = false;
      stopPlayback();
      callbacks.onVolumeLevel(0);
      if (wasInUse) callbacks.onInterruption?.();
    },
  );
  const playbackSubscription = native.addExpoTwoWayAudioEventListener(
    "onPlaybackComplete",
    (event) => {
      const item = pending.get(event.id);
      if (!item) return;
      pending.delete(event.id);
      if (event.error) item.reject(new Error(event.error));
      else item.resolve(item.duration);
      releaseSessionIfIdle();
    },
  );

  async function queueAudio(id: string, audio: AudioPlaybackSource, generation: number) {
    if (generation !== playbackGeneration || destroyed) return;
    await ensureInitialized();
    if (generation !== playbackGeneration || destroyed) {
      releaseSessionIfIdle();
      return;
    }
    const buffer = await audio.arrayBuffer();
    if (generation !== playbackGeneration || destroyed) return;
    if (!buffer.byteLength || buffer.byteLength % 2 || buffer.byteLength > 512 * 1024) {
      throw new Error("Invalid PCM audio length");
    }
    const inputRate = parsePcmSampleRate(audio.type || "") ?? 24000;
    if (![8000, 16000, 22050, 24000, 44100, 48000].includes(inputRate)) {
      throw new Error("Unsupported PCM sample rate");
    }
    const pcm = resamplePcm16(new Uint8Array(buffer), inputRate, 16000);
    const item = pending.get(id);
    if (!item) return;
    item.duration = pcm.length / 2 / 16000;
    native.resumePlayback();
    if (!native.enqueuePCMData(pcm, id)) throw new Error("Audio playback unavailable");
  }

  return {
    initialize: ensureInitialized,

    async destroy() {
      if (destroyed) return;
      destroyed = true;
      captureGeneration += 1;
      captureAttempt = null;
      stopPlayback();
      if (captureActive) native.toggleRecording(false);
      captureActive = false;
      muted = false;
      callbacks.onVolumeLevel(0);
      if (initialized) native.tearDown();
      initialized = false;
      microphoneSubscription.remove();
      volumeSubscription.remove();
      interruptionSubscription.remove();
      playbackSubscription.remove();
    },

    async startCapture() {
      if (destroyed || captureActive) return;
      const generation = ++captureGeneration;
      captureAttempt = generation;
      try {
        let permission = await native.getMicrophonePermissionsAsync().catch(() => null);
        if (generation !== captureGeneration || destroyed) return;
        if (!permission?.granted) {
          permission = await native.requestMicrophonePermissionsAsync().catch(() => null);
        }
        if (generation !== captureGeneration || destroyed) return;
        if (!permission?.granted)
          throw new Error("Microphone permission is required to capture audio.");
        await ensureInitialized();
        if (generation !== captureGeneration || destroyed) {
          releaseSessionIfIdle();
          return;
        }
        if (!native.toggleRecording(true)) throw new Error("Microphone capture could not start.");
        captureActive = true;
      } catch (error) {
        if (generation !== captureGeneration || destroyed) return;
        releaseSessionIfIdle();
        const wrapped = error instanceof Error ? error : new Error(String(error));
        callbacks.onError?.(wrapped);
        throw wrapped;
      } finally {
        if (captureAttempt === generation) {
          captureAttempt = null;
          releaseSessionIfIdle();
        }
      }
    },

    async stopCapture() {
      captureGeneration += 1;
      captureAttempt = null;
      if (captureActive) native.toggleRecording(false);
      captureActive = false;
      muted = false;
      callbacks.onVolumeLevel(0);
      releaseSessionIfIdle();
    },

    toggleMute() {
      muted = !muted;
      if (muted) callbacks.onVolumeLevel(0);
      return muted;
    },
    isMuted: () => muted,

    play(audio) {
      if (destroyed || pending.size >= 32)
        return Promise.reject(new Error("Audio playback unavailable"));
      const id = `${engineId}:${++nextPlaybackId}`;
      const generation = playbackGeneration;
      const result = new Promise<number>((resolve, reject) => {
        pending.set(id, { duration: 0, resolve, reject });
      });
      // Serialize decoding/submission only. Completion is driven by the device, never
      // a JS timer or a prerequisite for scheduling the next PCM block.
      decoding = decoding
        .then(() => queueAudio(id, audio, generation))
        .catch((error: unknown) => {
          const item = pending.get(id);
          if (!item) return;
          pending.delete(id);
          item.reject(error instanceof Error ? error : new Error(String(error)));
          releaseSessionIfIdle();
        });
      return result;
    },
    stop: stopPlayback,
    clearQueue: stopPlayback,
    isPlaying: () => pending.size > 0,
  };
}
