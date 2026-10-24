import { afterEach, expect, it, vi } from "vitest";
import { createRealtimeAudio } from "./realtime-audio.web";

function fixture() {
  const track = { enabled: true, stop: vi.fn(), onended: null };
  const microphone = { getTracks: () => [track], getAudioTracks: () => [track] };
  const getUserMedia = vi.fn().mockResolvedValue(microphone);
  const speaker = {
    autoplay: false,
    srcObject: null,
    pause: vi.fn(),
    play: vi.fn().mockResolvedValue(undefined),
  };
  const peer = {
    addTrack: vi.fn(),
    createDataChannel: vi.fn(),
    close: vi.fn(),
    createOffer: vi.fn().mockResolvedValue({ type: "offer", sdp: "offer" }),
    setLocalDescription: vi.fn().mockResolvedValue(undefined),
    setRemoteDescription: vi.fn().mockResolvedValue(undefined),
    localDescription: { sdp: "offer" },
    connectionState: "new",
    ontrack: null,
    onconnectionstatechange: null,
  };
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
  vi.stubGlobal("document", { createElement: () => speaker });
  vi.stubGlobal(
    "RTCPeerConnection",
    class {
      constructor() {
        return peer;
      }
    },
  );
  const callbacks = { onCaptureData: vi.fn(), onVolumeLevel: vi.fn(), onInterruption: vi.fn() };
  return {
    audio: createRealtimeAudio(callbacks),
    peer,
    track,
    speaker,
    getUserMedia,
    microphone,
    callbacks,
  };
}

afterEach(() => vi.unstubAllGlobals());

it("negotiates native audio, mutes its microphone and closes every media resource", async () => {
  const { audio, peer, track, speaker, microphone, callbacks } = fixture();
  expect(await audio.offer()).toBe("offer");
  expect(peer.addTrack).toHaveBeenCalledWith(track, microphone);
  await audio.answer("answer");
  expect(peer.setRemoteDescription).toHaveBeenCalledWith({ type: "answer", sdp: "answer" });
  expect(audio.toggleMute()).toBe(true);
  expect(track.enabled).toBe(false);
  expect(callbacks.onCaptureData).not.toHaveBeenCalled();
  audio.stop();
  expect(peer.close).toHaveBeenCalledOnce();
  expect(track.stop).toHaveBeenCalledOnce();
  expect(speaker.pause).toHaveBeenCalledOnce();
  expect(audio.isActive()).toBe(false);
});

it("releases a microphone granted after startup was cancelled", async () => {
  const { audio, getUserMedia, microphone, track, peer } = fixture();
  let grant!: (value: typeof microphone) => void;
  getUserMedia.mockReturnValue(
    new Promise((resolve) => {
      grant = resolve;
    }),
  );
  const pending = audio.offer();
  audio.stop();
  grant(microphone);
  await expect(pending).rejects.toThrow("Voice startup cancelled");
  expect(track.stop).toHaveBeenCalledOnce();
  expect(peer.addTrack).not.toHaveBeenCalled();
});
