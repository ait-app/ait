import { create } from "zustand";

interface ProviderSettingsTarget {
  serverId: string;
  provider: string;
  overlayParentLayer?: number;
  /** Directory whose preset store the sheet manages, when the caller knows one. */
  cwd?: string | null;
}

interface ProviderSettingsStoreState {
  serverId: string | null;
  provider: string | null;
  overlayParentLayer: number;
  cwd: string | null;
  visible: boolean;
  open: (target: ProviderSettingsTarget) => void;
  close: () => void;
}

export const useProviderSettingsStore = create<ProviderSettingsStoreState>()((set) => ({
  serverId: null,
  provider: null,
  overlayParentLayer: 0,
  cwd: null,
  visible: false,
  open: ({ serverId, provider, overlayParentLayer = 0, cwd = null }) => {
    set({ serverId, provider, overlayParentLayer, cwd, visible: true });
  },
  close: () => {
    set({ visible: false });
  },
}));
