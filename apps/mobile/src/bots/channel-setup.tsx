import React, { useEffect, useRef } from "react";
import { Pressable, Text } from "react-native";
import { useQuery } from "@tanstack/react-query";
import { botStyles as styles } from "./styles";

const STATUS_POLL_MS = 3000;

export function TextLink({
  label,
  onPress,
  testID,
}: {
  label: string;
  onPress: () => void;
  testID: string;
}) {
  return (
    <Pressable onPress={onPress} accessibilityRole="button" testID={testID}>
      <Text style={[styles.hint, styles.link]}>{label}</Text>
    </Pressable>
  );
}

/**
 * Poll a host-side setup session until it finishes. Leaving a live session cancels it, so a
 * later approval on the external platform cannot create an account with no channel.
 */
export function useLiveSetup<Status extends { state: "waiting" | "complete" | "failed" }>({
  queryKey,
  sessionId,
  getStatus,
  cancel,
  onDone,
}: {
  queryKey: readonly unknown[];
  sessionId: string | undefined;
  getStatus: (sessionId: string) => Promise<Status>;
  cancel: (sessionId: string) => void;
  onDone: () => void;
}) {
  const status = useQuery({
    queryKey: [...queryKey, sessionId],
    enabled: Boolean(sessionId),
    gcTime: 0,
    refetchInterval: (query) =>
      query.state.data && query.state.data.state !== "waiting" ? false : STATUS_POLL_MS,
    queryFn: () => {
      if (!sessionId) throw new Error("No setup session");
      return getStatus(sessionId);
    },
  });
  const latestCancel = useRef(cancel);
  latestCancel.current = cancel;
  const live = useRef<string | undefined>(undefined);
  useEffect(() => {
    live.current = sessionId;
  }, [sessionId]);
  const state = status.data?.state;
  useEffect(() => {
    if (state !== "complete" && state !== "failed") return;
    live.current = undefined;
    if (state === "complete") onDone();
  }, [state, onDone]);
  const stop = () => {
    if (live.current) latestCancel.current(live.current);
    live.current = undefined;
  };
  const stopRef = useRef(stop);
  stopRef.current = stop;
  useEffect(() => () => stopRef.current(), []);
  return { status: status.data, stop };
}
