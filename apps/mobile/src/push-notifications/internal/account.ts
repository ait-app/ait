import type { HubAccountConnection } from "@ait/client/internal/hub";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { startSubscription } from "./subscriptions";

const revokers = new Set<() => Promise<void>>();

export function startAccountPushNotifications(input: {
  connection: HubAccountConnection;
  key: string;
}): () => void {
  const { connection } = input;
  const key = `account:${input.key}`;
  let currentToken: string | null = null;
  let stop = () => {};
  const connected = () =>
    connection.snapshot().state === "online" &&
    connection.snapshot().methods.includes("push.register");
  const revoke = async () => {
    stop();
    const token = currentToken ?? (await AsyncStorage.getItem(`@ait:native-push-token:${key}`));
    if (token && connected()) await connection.call("push.unregister.request", { token });
    await AsyncStorage.removeItem(`@ait:native-push-token:${key}`);
  };
  revokers.add(revoke);
  stop = startSubscription({
    serverId: key,
    client: {
      get isConnected() {
        return connected();
      },
      registerPushToken: (token) => {
        currentToken = token;
        return connection.call("push.register", { token }).then(() => {});
      },
      unregisterPushToken: (token) =>
        connection.call("push.unregister.request", { token }).then(() => {}),
      subscribeConnectionStatus(listener) {
        const emit = () => listener({ status: connected() ? "connected" : "disconnected" });
        emit();
        return connection.subscribe(emit);
      },
    },
  });
  return () => {
    revokers.delete(revoke);
    stop();
  };
}

/** Explicit logout revokes the account target before releasing the authenticated link. */
export async function revokeAccountPushNotifications(): Promise<void> {
  await Promise.all(
    Array.from(revokers, (revoke) =>
      revoke().catch(() => {
        console.warn("[PushNotifications] Account push revocation failed");
      }),
    ),
  );
}
