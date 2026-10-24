import { afterEach, expect, it, vi } from "vitest";
import { createRealtimeAudio } from "./realtime-audio.web";

function fixture() {
  const track = { enabled: true, stop: vi.fn(), onended: null as (() => void) | null };
  const channel = { onclose: null as (() => void) | null };
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
    createDataChannel: vi.fn().mockReturnValue(channel),
    close: vi.fn(),
    createOffer: vi.fn().mockResolvedValue({ type: "offer", sdp: "offer" }),
    setLocalDescription: vi.fn().mockResolvedValue(undefined),
    setRemoteDescription: vi.fn().mockResolvedValue(undefined),
    localDescription: { sdp: "offer" },
    connectionState: "new",
    ontrack: null as ((event: { streams: unknown[]; track: typeof track }) => void) | null,
    onconnectionstatechange: null as (() => void) | null,
  };
  const createPeer = vi.fn().mockReturnValue(peer);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
  vi.stubGlobal("document", { createElement: () => speaker });
  vi.stubGlobal(
    "RTCPeerConnection",
    class {
      constructor() {
        return createPeer();
      }
    },
  );
  const callbacks = { onCaptureData: vi.fn(), onVolumeLevel: vi.fn(), onInterruption: vi.fn() };
  return {
    audio: createRealtimeAudio(callbacks),
    peer,
    createPeer,
    channel,
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
  peer.connectionState = "closed";
  peer.onconnectionstatechange?.();
  expect(callbacks.onInterruption).not.toHaveBeenCalled();
});

it("ignores delayed close and playback errors from a previous call", async () => {
  const { audio, peer, createPeer, channel, track, speaker, callbacks } = fixture();
  await audio.offer();
  let rejectPlayback!: (error: Error) => void;
  speaker.play.mockReturnValue(
    new Promise((_, reject) => {
      rejectPlayback = reject;
    }),
  );
  peer.ontrack?.({ streams: [{}], track });
  const previousChannelClosed = channel.onclose;
  const previousTrackEnded = track.onended;
  createPeer.mockReturnValue({ ...peer });
  await audio.offer();

  previousChannelClosed?.();
  previousTrackEnded?.();
  peer.connectionState = "closed";
  peer.onconnectionstatechange?.();
  rejectPlayback(new Error("Playback aborted"));
  await Promise.resolve();

  expect(audio.isActive()).toBe(true);
  expect(track.stop).toHaveBeenCalledOnce();
  expect(callbacks.onInterruption).not.toHaveBeenCalled();
  audio.stop();
});

it.each(["channel closed", "peer closed", "peer failed", "remote track ended"])(
  "stops microphone and playback when Codex exits through %s",
  async (reason) => {
    const { audio, peer, channel, track, speaker, callbacks } = fixture();
    await audio.offer();
    const remote = { enabled: true, stop: vi.fn(), onended: null as (() => void) | null };
    const remoteStream = {};
    peer.ontrack?.({ streams: [remoteStream], track: remote });
    expect(speaker.srcObject).toBe(remoteStream);

    if (reason === "channel closed") channel.onclose?.();
    else if (reason === "remote track ended") remote.onended?.();
    else {
      peer.connectionState = reason === "peer closed" ? "closed" : "failed";
      peer.onconnectionstatechange?.();
    }

    expect(audio.isActive()).toBe(false);
    expect(track.stop).toHaveBeenCalledOnce();
    expect(speaker.pause).toHaveBeenCalledOnce();
    expect(speaker.srcObject).toBeNull();
    expect(callbacks.onInterruption).toHaveBeenCalledOnce();
    channel.onclose?.();
    peer.onconnectionstatechange?.();
    remote.onended?.();
    expect(callbacks.onInterruption).toHaveBeenCalledOnce();
  },
);

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
