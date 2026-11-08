import { UnistylesRuntime } from "react-native-unistyles";

/** Reconcile retained React theme snapshots with the browser's current system palette. */
export function subscribeToSystemTheme(): () => void {
  const media = window.matchMedia("(prefers-color-scheme: dark)");

  const synchronize = () => {
    if (document.visibilityState === "hidden" || !UnistylesRuntime.hasAdaptiveThemes) return;
    const themeName = UnistylesRuntime.themeName;
    if (!themeName) return;

    // CSS media queries can switch while a retained React tree misses the theme notification.
    // Re-publish even when the name matches the last visible palette: consumers may have
    // received only part of a hidden-page round trip and retained the intermediate theme.
    // A normal theme notification has already supplied this same object, so React bails out.
    UnistylesRuntime.updateTheme(themeName, (theme) => theme);
  };

  media.addEventListener("change", synchronize);
  document.addEventListener("visibilitychange", synchronize);
  window.addEventListener("focus", synchronize);
  window.addEventListener("pageshow", synchronize);

  return () => {
    media.removeEventListener("change", synchronize);
    document.removeEventListener("visibilitychange", synchronize);
    window.removeEventListener("focus", synchronize);
    window.removeEventListener("pageshow", synchronize);
  };
}
