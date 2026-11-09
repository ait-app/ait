import type { ReactNode } from "react";
import { create } from "zustand";

interface ProviderSettingsTarget {
  serverId: string;
  provider: string;
  overlayParentLayer?: number;
  configuration?: ReactNode;
  sourceId?: string;
  /** Directory whose preset store the sheet manages, when the caller knows one. */
  cwd?: string | null;
}

interface ProviderSettingsStoreState {
  serverId: string | null;
  provider: string | null;
  overlayParentLayer: number;
  cwd: string | null;
  visible: boolean;
  configuration: ReactNode;
  sourceId: string | null;
  updateConfiguration: (sourceId: string, configuration: ReactNode) => void;
  open: (target: ProviderSettingsTarget) => void;
  close: () => void;
}

export const useProviderSettingsStore = create<ProviderSettingsStoreState>()((set) => ({
  serverId: null,
  provider: null,
  overlayParentLayer: 0,
  cwd: null,
  visible: false,
  configuration: null,
  sourceId: null,
  updateConfiguration: (sourceId, configuration) => {
    set((state) => (state.visible && state.sourceId === sourceId ? { configuration } : state));
  },
  open: ({
    serverId,
    provider,
    overlayParentLayer = 0,
    cwd = null,
    configuration = null,
    sourceId = null,
  }) => {
    set({ serverId, provider, overlayParentLayer, cwd, configuration, sourceId, visible: true });
  },
  close: () => {
    set({ visible: false, configuration: null, sourceId: null });
  },
}));
