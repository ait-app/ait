export interface AudioEngineCallbacks {
  onCaptureData(pcm: Uint8Array): void;
  onVolumeLevel(level: number): void;
  onInterruption?(): void;
  onError?(error: Error): void;
}

export interface AudioPlaybackSource {
  arrayBuffer(): Promise<ArrayBuffer>;
  size: number;
  type: string;
}

export interface AudioEngine {
  initialize(): Promise<void>;
  destroy(): Promise<void>;

  /** Optional native WebRTC audio; provider authentication stays on the host. */
  createRealtimeOffer?(): Promise<string>;
  acceptRealtimeAnswer?(sdp: string): Promise<void>;
  startCapture(): Promise<void>;
  stopCapture(): Promise<void>;
  toggleMute(): boolean;
  isMuted(): boolean;

  play(audio: AudioPlaybackSource): Promise<number>;
  stop(): void;
  clearQueue(): void;
  isPlaying(): boolean;
}
