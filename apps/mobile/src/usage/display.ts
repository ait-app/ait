import { useCallback, useMemo } from "react";
import { useAppSettings } from "@/hooks/use-settings";
import { useUsageHostId } from "./hosts";
import { useUsageHostReports } from "./queries";
import {
  isUsagePinned,
  setUsageDisplayAs,
  toggleUsagePin,
  type UsageDisplayAs,
  type UsagePin,
  type UsagePreferences,
} from "./preferences";
import type { UsageReportEntry } from "./types";

/** What usage views need from the preferences: how to read percents, and the pins. */
export interface UsageDisplay {
  displayAs: UsageDisplayAs;
  setDisplayAs: (displayAs: UsageDisplayAs) => void;
  isPinned: (pin: UsagePin) => boolean;
  togglePin: (pin: UsagePin) => void;
}

interface UsagePreferencesState {
  preferences: UsagePreferences;
  display: UsageDisplay;
}

/** `useAppSettings().updateSettings` logs a failed save before rethrowing; nothing else to do. */
function ignoreLoggedSaveFailure(): void {}

/** The device's usage preferences, with pins resolved against a host's reports. */
export function useUsagePreferences(serverId?: string): UsagePreferencesState {
  const defaultServerId = useUsageHostId();
  return useUsageDisplay(useUsageHostReports(serverId ?? defaultServerId));
}

/** The device's usage preferences, with pins resolved against the given reports. */
export function useUsageDisplay(reports: readonly UsageReportEntry[]): UsagePreferencesState {
  const { settings, updateSettings } = useAppSettings();
  const preferences = settings.usage;
  const setDisplayAs = useCallback(
    (displayAs: UsageDisplayAs) => {
      updateSettings((current) => ({
        usage: setUsageDisplayAs(current.usage, displayAs),
      })).catch(ignoreLoggedSaveFailure);
    },
    [updateSettings],
  );
  const togglePin = useCallback(
    (pin: UsagePin) => {
      updateSettings((current) => ({
        usage: toggleUsagePin({ preferences: current.usage, pin, reports }),
      })).catch(ignoreLoggedSaveFailure);
    },
    [updateSettings, reports],
  );
  const display = useMemo<UsageDisplay>(
    () => ({
      displayAs: preferences.displayAs,
      setDisplayAs,
      isPinned: (pin) => isUsagePinned(preferences, pin, reports),
      togglePin,
    }),
    [preferences, reports, setDisplayAs, togglePin],
  );
  return { preferences, display };
}
