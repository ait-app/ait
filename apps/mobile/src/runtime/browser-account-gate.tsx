import React, { useEffect, useState, type ReactNode } from "react";
import { Button, Platform, Text, View } from "react-native";
import { getDesktopHost } from "@/desktop/host";
import {
  getBrowserAccount,
  subscribeBrowserAccount,
  supportsBrowserAccount,
} from "./browser-account";

/** The hosted website requires an account; native and desktop clients also support local hosts. */
export function BrowserAccountGate({ children }: { children: ReactNode }) {
  const enabled = Platform.OS === "web" && supportsBrowserAccount() && !getDesktopHost()?.invoke;
  const [authorized, setAuthorized] = useState(false);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let leaving = false;
    let redirected = false;
    const redirect = () => {
      if (disposed || leaving || redirected) return;
      redirected = true;
      setAuthorized(false);
      window.location.replace("/login?client=1");
    };
    const receive = (state: { status: string }) => {
      if (disposed || leaving || redirected) return;
      if (state.status === "logged_out") redirect();
      else setAuthorized(true);
    };
    // shutdown() publishes logged_out while leaving, without signing the user out.
    const onPageHide = () => {
      leaving = true;
    };
    const onStorage = (event: StorageEvent) => {
      if (
        event.storageArea === window.localStorage &&
        (event.key === "ait.access-token" || event.key === null)
      ) {
        // Block old account content immediately; the account manager reloads changed logins.
        setAuthorized(false);
        if (!window.localStorage.getItem("ait.access-token")) redirect();
        else leaving = true;
      }
    };
    window.addEventListener("pagehide", onPageHide);
    window.addEventListener("storage", onStorage);
    const remove = subscribeBrowserAccount(receive);
    setFailed(false);
    void getBrowserAccount()
      .then((manager) => receive(manager.snapshot()))
      .catch(() => {
        if (!disposed && !leaving) setFailed(true);
      });
    return () => {
      disposed = true;
      remove();
      window.removeEventListener("pagehide", onPageHide);
      window.removeEventListener("storage", onStorage);
    };
  }, [enabled, attempt]);

  if (!enabled || authorized) return children;
  if (!failed) return null;
  return (
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 12 }}>
      <Text>Could not connect to the account service. Please try again.</Text>
      <Button title="Retry" onPress={() => setAttempt((value) => value + 1)} />
    </View>
  );
}
