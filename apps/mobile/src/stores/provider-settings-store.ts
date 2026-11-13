import { create } from "zustand";

interface ProviderSettingsTarget {
  serverId: string;
  provider: string;
  cwd?: string;
  overlayParentLayer?: number;
}

interface ProviderSettingsStoreState {
  serverId: string | null;
  provider: string | null;
  cwd: string | undefined;
  overlayParentLayer: number;
  visible: boolean;
  open: (target: ProviderSettingsTarget) => void;
  close: () => void;
}

export const useProviderSettingsStore = create<ProviderSettingsStoreState>()((set) => ({
  serverId: null,
  provider: null,
  cwd: undefined,
  overlayParentLayer: 0,
  visible: false,
  open: ({ serverId, provider, cwd, overlayParentLayer = 0 }) => {
    set({ serverId, provider, cwd, overlayParentLayer, visible: true });
  },
  close: () => {
    set({ visible: false });
  },
}));
