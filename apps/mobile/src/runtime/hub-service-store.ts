import { create } from "zustand";
import type { HubAccountConnection, HubAccountSnapshot } from "@ait/client/internal/hub";
import { emptyHubDirectory, type HubDirectory } from "./hub-services";

interface HubServicesState {
  connection: HubAccountConnection | null;
  snapshot: HubAccountSnapshot | null;
  directory: HubDirectory;
  loading: boolean;
  eventsActive: boolean;
  error: boolean;
  refresh(): Promise<void>;
}
export const useHubServices = create<HubServicesState>(() => ({
  connection: null,
  snapshot: null,
  directory: emptyHubDirectory(),
  loading: false,
  eventsActive: false,
  error: false,
  refresh: async () => {},
}));
