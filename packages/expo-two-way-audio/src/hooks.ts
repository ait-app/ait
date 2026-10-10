import { createPermissionHook } from "expo-modules-core";
import { useEffect, useSyncExternalStore } from "react";
import {
  getMicrophonePermissionsAsync,
  isRecording,
  requestMicrophonePermissionsAsync,
} from "./core";
import { ExpoTwoWayAudioEventMap, addExpoTwoWayAudioEventListener } from "./events";

export const useMicrophonePermissions = createPermissionHook({
  getMethod: getMicrophonePermissionsAsync,
  requestMethod: requestMicrophonePermissionsAsync,
});

function subscribeToRecordingChange(onChange: () => void) {
  const subscription = addExpoTwoWayAudioEventListener("onRecordingChange", onChange);
  return () => subscription.remove();
}

const getServerRecordingSnapshot = () => false;

export function useIsRecording() {
  return useSyncExternalStore(subscribeToRecordingChange, isRecording, getServerRecordingSnapshot);
}

export function useExpoTwoWayAudioEventListener<K extends keyof ExpoTwoWayAudioEventMap>(
  eventName: K,
  listener: (ev: ExpoTwoWayAudioEventMap[K]) => void,
) {
  useEffect(() => {
    const sub = addExpoTwoWayAudioEventListener(eventName, listener);
    return () => sub.remove();
  }, [eventName, listener]);
}
