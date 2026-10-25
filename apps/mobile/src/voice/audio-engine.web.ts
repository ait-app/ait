import { isElectronRuntime } from "@/desktop/host";
import type {
  AudioEngine,
  AudioEngineCallbacks,
  AudioPlaybackSource,
} from "@/voice/audio-engine-types";

interface QueuedAudio {
  audio: AudioPlaybackSource;
  resolve: (duration: number) => void;
  reject: (error: Error) => void;
}

function getAudioContextCtor(): typeof AudioContext | null {
  if (typeof window === "undefined") {
    return null;
  }
  const browserWindow = window as typeof window & {
    webkitAudioContext?: typeof AudioContext;
  };
  return browserWindow.AudioContext ?? browserWindow.webkitAudioContext ?? null;
}

function floatToInt16(sample: number): number {
  const clamped = Math.max(-1, Math.min(1, sample));
  return clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff);
}

function resampleToPcm16(input: Float32Array, inputRate: number, outputRate: number): Uint8Array {
  if (input.length === 0) {
    return new Uint8Array(0);
  }

  const ratio = inputRate / outputRate;
  const outputLength = Math.max(1, Math.round(input.length / ratio));
  const output = new Int16Array(outputLength);
  for (let i = 0; i < outputLength; i += 1) {
    const sourceIndex = i * ratio;
    const i0 = Math.floor(sourceIndex);
    const i1 = Math.min(input.length - 1, i0 + 1);
    const frac = sourceIndex - i0;
    const sample = input[i0] * (1 - frac) + input[i1] * frac;
    output[i] = floatToInt16(sample);
  }

  return new Uint8Array(output.buffer, output.byteOffset, output.byteLength);
}

function parsePcmSampleRate(mimeType: string): number | null {
  const match = /rate=(\d+)/i.exec(mimeType);
  if (!match) {
    return null;
  }
  const rate = Number(match[1]);
  return Number.isFinite(rate) && rate > 0 ? rate : null;
}

function pcm16LeToAudioBuffer(
  context: AudioContext,
  bytes: Uint8Array,
  sampleRate: number,
): AudioBuffer {
  const sampleCount = Math.floor(bytes.length / 2);
  const audioBuffer = context.createBuffer(1, sampleCount, sampleRate);
  const channel = audioBuffer.getChannelData(0);
  for (let i = 0; i < sampleCount; i += 1) {
    const lo = bytes[i * 2];
    const hi = bytes[i * 2 + 1];
    let value = (hi << 8) | lo;
    if (value & 0x8000) {
      value -= 0x10000;
    }
    channel[i] = value / 0x8000;
  }
  return audioBuffer;
}

async function decodeAudioData(context: AudioContext, buffer: ArrayBuffer): Promise<AudioBuffer> {
  const maybePromise = context.decodeAudioData(buffer.slice(0));
  if (maybePromise && typeof maybePromise.then === "function") {
    return maybePromise;
  }
  return await new Promise<AudioBuffer>((resolve, reject) => {
    context.decodeAudioData(buffer.slice(0), resolve, reject);
  });
}

export function createAudioEngine(
  callbacks: AudioEngineCallbacks,
  _options?: { traceLabel?: string },
): AudioEngine {
  const refs: {
    playbackContext: AudioContext | null;
    captureContext: AudioContext | null;
    stream: MediaStream | null;
    source: MediaStreamAudioSourceNode | null;
    processor: ScriptProcessorNode | null;
    gain: GainNode | null;
    started: boolean;
    captureGeneration: number;
    muted: boolean;
    queue: QueuedAudio[];
    processingQueue: boolean;
    playbackGeneration: number;
    nextPlaybackTime: number;
    consecutiveSilence: number;
    decoding: QueuedAudio | null;
    activePlayback: Map<AudioBufferSourceNode, QueuedAudio>;
  } = {
    playbackContext: null,
    captureContext: null,
    stream: null,
    source: null,
    processor: null,
    gain: null,
    started: false,
    captureGeneration: 0,
    muted: false,
    queue: [],
    processingQueue: false,
    playbackGeneration: 0,
    nextPlaybackTime: 0,
    consecutiveSilence: 0,
    decoding: null,
    activePlayback: new Map(),
  };

  async function ensurePlaybackContext(): Promise<AudioContext> {
    if (refs.playbackContext) {
      if (refs.playbackContext.state === "suspended") {
        await refs.playbackContext.resume().catch(() => undefined);
      }
      return refs.playbackContext;
    }

    const AudioContextCtor = getAudioContextCtor();
    if (!AudioContextCtor) {
      throw new Error("AudioContext unavailable");
    }

    // Native realtime PCM is 24 kHz. Resample the continuous output graph at the
    // device boundary, rather than independently resampling every short packet.
    const context = new AudioContextCtor({ sampleRate: 24000 });
    refs.playbackContext = context;
    if (context.state === "suspended") {
      await context.resume().catch(() => undefined);
    }
    return context;
  }

  async function ensureCaptureContext(): Promise<AudioContext> {
    if (refs.captureContext) {
      if (refs.captureContext.state === "suspended") {
        await refs.captureContext.resume().catch(() => undefined);
      }
      return refs.captureContext;
    }

    const AudioContextCtor = getAudioContextCtor();
    if (!AudioContextCtor) {
      throw new Error("AudioContext unavailable");
    }

    const context = new AudioContextCtor();
    refs.captureContext = context;
    if (context.state === "suspended") {
      await context.resume().catch(() => undefined);
    }
    return context;
  }

  async function scheduleAudio(item: QueuedAudio, generation: number): Promise<void> {
    const context = await ensurePlaybackContext();
    const arrayBuffer = await item.audio.arrayBuffer();
    const type = (item.audio.type || "").toLowerCase();
    const audioBuffer = type.startsWith("audio/pcm")
      ? pcm16LeToAudioBuffer(
          context,
          new Uint8Array(arrayBuffer),
          parsePcmSampleRate(type) ?? 24000,
        )
      : await decodeAudioData(context, arrayBuffer);

    if (generation !== refs.playbackGeneration) throw new Error("Playback stopped");
    const silent =
      type.startsWith("audio/pcm") && audioBuffer.getChannelData(0).every((sample) => sample === 0);
    refs.consecutiveSilence = silent ? refs.consecutiveSilence + audioBuffer.duration : 0;
    // Reliable transport can deliver old silence in a burst after a stall. Keep
    // short pauses, but consume excess all-zero padding without extending delay.
    // Never trim nonzero samples, even very quiet speech or the start of a reply.
    if (refs.consecutiveSilence > 0.24 && refs.nextPlaybackTime - context.currentTime > 0.24) {
      item.resolve(0);
      return;
    }
    const source = context.createBufferSource();
    source.buffer = audioBuffer;
    source.connect(context.destination);
    refs.activePlayback.set(source, item);
    source.addEventListener("ended", () => {
      if (!refs.activePlayback.delete(source)) return;
      source.disconnect();
      item.resolve(audioBuffer.duration);
    });

    // Prime once with a short lead. After an underrun resume promptly instead
    // of adding another full startup delay each time the input stream pauses.
    // Continuous packets share the audio clock, independent of ended delivery.
    const startAt =
      refs.nextPlaybackTime > context.currentTime
        ? refs.nextPlaybackTime
        : context.currentTime + (refs.nextPlaybackTime === 0 ? 0.12 : 0.02);
    try {
      source.start(startAt);
      refs.nextPlaybackTime = startAt + audioBuffer.duration;
    } catch (error) {
      refs.activePlayback.delete(source);
      source.disconnect();
      throw error;
    }
  }

  async function processQueue(): Promise<void> {
    if (refs.processingQueue || refs.queue.length === 0) return;
    refs.processingQueue = true;
    const generation = refs.playbackGeneration;
    try {
      while (generation === refs.playbackGeneration && refs.queue.length > 0) {
        const item = refs.queue.shift()!;
        refs.decoding = item;
        try {
          // Serialize decoding/scheduling, not playback completion. play() still
          // resolves only after this packet has actually been consumed.
          await scheduleAudio(item, generation);
        } catch (error) {
          item.reject(error instanceof Error ? error : new Error(String(error)));
        }
        if (refs.decoding === item) refs.decoding = null;
      }
    } finally {
      if (generation === refs.playbackGeneration) refs.processingQueue = false;
    }
  }

  async function stopCapture(): Promise<void> {
    refs.captureGeneration += 1;
    refs.started = false;

    try {
      refs.processor?.disconnect();
      refs.source?.disconnect();
      refs.gain?.disconnect();
    } catch {
      // Ignore best-effort teardown errors.
    }

    if (refs.stream) {
      for (const track of refs.stream.getTracks()) {
        try {
          track.onended = null;
          track.stop();
        } catch {
          // Ignore best-effort teardown errors.
        }
      }
    }

    refs.stream = null;
    refs.source = null;
    refs.processor = null;
    refs.gain = null;
    refs.muted = false;
    callbacks.onVolumeLevel(0);

    const captureContext = refs.captureContext;
    refs.captureContext = null;
    if (captureContext && captureContext.state !== "closed") {
      await captureContext.close().catch(() => undefined);
    }
  }

  return {
    async initialize() {
      await ensurePlaybackContext();
    },

    async destroy() {
      this.stop();
      this.clearQueue();
      await stopCapture();

      const playbackContext = refs.playbackContext;
      refs.playbackContext = null;
      if (playbackContext && playbackContext.state !== "closed") {
        await playbackContext.close().catch(() => undefined);
      }
    },

    async startCapture() {
      if (refs.started) {
        return;
      }

      const missingNavigator =
        typeof navigator === "undefined" ||
        !navigator.mediaDevices ||
        typeof navigator.mediaDevices.getUserMedia !== "function";
      const secureContext =
        typeof window !== "undefined" && typeof window.isSecureContext === "boolean"
          ? window.isSecureContext
          : true;
      const currentOrigin =
        typeof window !== "undefined" && window.location ? window.location.origin : "unknown";
      const isDesktopApp = isElectronRuntime();

      if (missingNavigator) {
        throw new Error("Microphone capture is not supported in this environment");
      }
      if (!secureContext && !isDesktopApp) {
        throw new Error(
          `Microphone access requires HTTPS or localhost. Current origin: ${currentOrigin}`,
        );
      }

      const generation = ++refs.captureGeneration;
      refs.started = true;
      try {
        const context = await ensureCaptureContext();
        if (generation !== refs.captureGeneration) return;
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: {
            channelCount: 1,
            noiseSuppression: true,
            echoCancellation: true,
            autoGainControl: true,
          },
        });
        if (generation !== refs.captureGeneration) {
          for (const track of stream.getTracks()) track.stop();
          return;
        }
        refs.stream = stream;
        for (const track of stream.getAudioTracks()) {
          track.onended = () => {
            if (generation !== refs.captureGeneration) return;
            void stopCapture();
            callbacks.onInterruption?.();
          };
        }
        const source = context.createMediaStreamSource(stream);
        const processor = context.createScriptProcessor(2048, 1, 1);
        const gain = context.createGain();
        gain.gain.value = 0;

        processor.onaudioprocess = (event) => {
          if (!refs.started) {
            return;
          }

          const input = event.inputBuffer.getChannelData(0);
          let sumSquares = 0;
          for (let i = 0; i < input.length; i += 1) {
            const sample = input[i];
            sumSquares += sample * sample;
          }
          const rms = Math.sqrt(sumSquares / Math.max(1, input.length));
          const normalized = Math.min(1, Math.max(0, rms * 2));
          callbacks.onVolumeLevel(normalized);

          if (refs.muted) {
            return;
          }

          callbacks.onCaptureData(resampleToPcm16(input, context.sampleRate, 16000));
        };

        source.connect(processor);
        processor.connect(gain);
        gain.connect(context.destination);

        refs.started = true;
        refs.stream = stream;
        refs.source = source;
        refs.processor = processor;
        refs.gain = gain;
      } catch (error) {
        if (generation !== refs.captureGeneration) return;
        await stopCapture();
        const wrapped = error instanceof Error ? error : new Error(String(error));
        callbacks.onError?.(wrapped);
        throw wrapped;
      }
    },

    async stopCapture() {
      await stopCapture();
    },

    toggleMute() {
      refs.muted = !refs.muted;
      if (refs.muted) {
        callbacks.onVolumeLevel(0);
      }
      return refs.muted;
    },

    isMuted() {
      return refs.muted;
    },

    async play(audio: AudioPlaybackSource) {
      return await new Promise<number>((resolve, reject) => {
        refs.queue.push({ audio, resolve, reject });
        if (!refs.processingQueue) {
          void processQueue();
        }
      });
    },

    stop() {
      refs.playbackGeneration += 1;
      refs.processingQueue = false;
      refs.nextPlaybackTime = 0;
      refs.consecutiveSilence = 0;
      refs.decoding?.reject(new Error("Playback stopped"));
      refs.decoding = null;
      for (const [source, item] of refs.activePlayback) {
        try {
          source.stop();
        } catch {
          // Ignore best-effort stop errors.
        }
        source.disconnect();
        item.reject(new Error("Playback stopped"));
      }
      refs.activePlayback.clear();
      this.clearQueue();
    },

    clearQueue() {
      while (refs.queue.length > 0) {
        refs.queue.shift()!.reject(new Error("Playback stopped"));
      }
    },

    isPlaying() {
      return refs.activePlayback.size > 0;
    },
  };
}
