import type { AudioEngineCallbacks } from "./audio-engine-types";

/** Browser audio transport. The provider owns authentication and SDP negotiation. */
export function createRealtimeAudio(callbacks: AudioEngineCallbacks) {
  let peer: RTCPeerConnection | null = null;
  let stream: MediaStream | null = null;
  let speaker: HTMLAudioElement | null = null;
  let muted = false;
  let generation = 0;

  function stop() {
    generation += 1;
    const previous = peer;
    peer = null;
    previous?.close();
    stream?.getTracks().forEach((track) => track.stop());
    stream = null;
    if (speaker) {
      speaker.pause();
      speaker.srcObject = null;
      speaker = null;
    }
    muted = false;
    callbacks.onVolumeLevel(0);
  }

  function interrupt(connection: RTCPeerConnection) {
    if (peer !== connection) return;
    stop();
    callbacks.onInterruption?.();
  }

  return {
    isActive: () => peer !== null,
    isMuted: () => muted,
    toggleMute() {
      muted = !muted;
      stream?.getAudioTracks().forEach((track) => {
        track.enabled = !muted;
      });
      return muted;
    },
    stop,
    async offer(): Promise<string> {
      stop();
      const attempt = generation;
      try {
        const microphone = await navigator.mediaDevices.getUserMedia({
          audio: {
            channelCount: 1,
            echoCancellation: true,
            noiseSuppression: true,
            autoGainControl: true,
          },
        });
        if (attempt !== generation) {
          microphone.getTracks().forEach((track) => track.stop());
          throw new Error("Voice startup cancelled");
        }
        stream = microphone;
        const connection = new RTCPeerConnection();
        peer = connection;
        const output = document.createElement("audio");
        output.autoplay = true;
        speaker = output;
        connection.ontrack = (event) => {
          if (peer !== connection) return;
          event.track.onended = () => interrupt(connection);
          output.srcObject = event.streams[0] ?? new MediaStream([event.track]);
          void output.play().catch((error: unknown) => {
            if (peer !== connection) return;
            callbacks.onError?.(error instanceof Error ? error : new Error(String(error)));
            interrupt(connection);
          });
        };
        connection.onconnectionstatechange = () => {
          if (connection.connectionState === "failed" || connection.connectionState === "closed") {
            interrupt(connection);
          }
        };
        microphone.getAudioTracks().forEach((track) => {
          track.onended = () => interrupt(connection);
          connection.addTrack(track, microphone);
        });
        const channel = connection.createDataChannel("oai-events");
        channel.onclose = () => interrupt(connection);
        await connection.setLocalDescription(await connection.createOffer());
        if (peer !== connection) throw new Error("Voice startup cancelled");
        const sdp = connection.localDescription?.sdp;
        if (!sdp) throw new Error("Could not create a realtime audio offer");
        return sdp;
      } catch (error) {
        if (attempt === generation) stop();
        throw error;
      }
    },
    async answer(sdp: string): Promise<void> {
      const connection = peer;
      if (!connection) throw new Error("Realtime audio session is closed");
      await connection.setRemoteDescription({ type: "answer", sdp });
    },
  };
}
