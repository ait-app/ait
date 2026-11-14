import { useCallback } from "react";
import { useWindowDimensions } from "react-native";
import { isNative } from "@/constants/platform";
import { useActiveWorkspaceSelection } from "@/stores/navigation-active-workspace-store";
import { selectIsCompactFileExplorerOpen, usePanelStore } from "@/stores/panel-store";
import { shouldYieldExplorerToNavigation } from "@/components/desktop-sidebar-layout";

/** Keep explicit navigation reachable when the native explorer occupies its space. */
export function useToggleAgentList(isCompact: boolean) {
  const { width } = useWindowDimensions();
  const activeWorkspace = useActiveWorkspaceSelection();
  const explorerOpen = usePanelStore(selectIsCompactFileExplorerOpen);
  const sidebarWidth = usePanelStore((state) => state.sidebarWidth);
  const toggleForLayout = usePanelStore((state) => state.toggleAgentListForLayout);
  const showAgent = usePanelStore((state) => state.showMobileAgent);
  const openNavigation = usePanelStore((state) => state.openDesktopAgentList);
  return useCallback(() => {
    if (
      shouldYieldExplorerToNavigation({
        isCompact,
        nativeWorkspaceActive: isNative && activeWorkspace !== null,
        explorerOpen,
        requestedSidebarWidth: sidebarWidth,
        viewportWidth: width,
      })
    ) {
      showAgent();
      openNavigation();
      return;
    }
    toggleForLayout({ isCompact });
  }, [
    isCompact,
    activeWorkspace,
    explorerOpen,
    sidebarWidth,
    width,
    showAgent,
    openNavigation,
    toggleForLayout,
  ]);
}
