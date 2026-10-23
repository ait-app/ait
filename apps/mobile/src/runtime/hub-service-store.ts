import { create } from "zustand";
import type {
  HubAccountConnection,
  HubAccountSnapshot,
  HubDirectoryItem,
} from "@ait/client/internal/hub";

interface HubServicesState {
  connection: HubAccountConnection | null;
  snapshot: HubAccountSnapshot | null;
  agents: HubDirectoryItem[];
  eventsActive: boolean;
}
export const useHubServices = create<HubServicesState>(() => ({
  connection: null,
  snapshot: null,
  agents: [],
  eventsActive: false,
}));
