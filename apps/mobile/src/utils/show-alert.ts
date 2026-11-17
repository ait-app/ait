import { Alert } from "react-native";
import { isNative } from "@/constants/platform";

export interface AlertNotice {
  title: string;
  message?: string;
}

type AlertPresenter = (notice: AlertNotice) => void;

let activePresenter: AlertPresenter | null = null;

/**
 * Registers the surface that shows alerts outside native. Returns a function that removes it
 * again, unless a newer presenter has replaced it in the meantime.
 */
export function setAlertPresenter(presenter: AlertPresenter): () => void {
  activePresenter = presenter;
  return () => {
    if (activePresenter === presenter) {
      activePresenter = null;
    }
  };
}

export function formatAlertNotice(notice: AlertNotice): string {
  const message = notice.message?.trim();
  if (!message) {
    return notice.title;
  }
  return `${notice.title}\n${message}`;
}

/**
 * Tells the user about something that needs no decision. `Alert.alert` from react-native-web is
 * an empty function, so calling it directly shows nothing in the browser or the desktop app.
 */
export function showAlert(title: string, message?: string): void {
  if (isNative) {
    Alert.alert(title, message);
    return;
  }

  const notice: AlertNotice = { title, message };
  if (activePresenter) {
    activePresenter(notice);
    return;
  }

  const browserAlert = (globalThis as { alert?: (message?: string) => void }).alert;
  if (typeof browserAlert === "function") {
    browserAlert(formatAlertNotice(notice));
    return;
  }
  console.error("[showAlert] No alert surface is available.", notice);
}
