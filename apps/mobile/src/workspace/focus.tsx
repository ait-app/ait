import React, { createContext, useMemo, type ReactNode } from "react";
import { useWorkspaceLayoutStore } from "@/stores/workspace-layout-store";

interface WorkspaceFocusContextValue {
  workspaceKey: string | null;
  unfocusPane: (workspaceKey: string) => string | null;
  restorePaneFocus: (workspaceKey: string, token: string) => void;
}

const WorkspaceFocusContext = createContext<WorkspaceFocusContextValue | null>(null);

export function WorkspaceFocusProvider({
  workspaceKey,
  children,
}: {
  workspaceKey: string | null;
  children: ReactNode;
}) {
  const unfocusPane = useWorkspaceLayoutStore((state) => state.unfocusPane);
  const restorePaneFocus = useWorkspaceLayoutStore((state) => state.restorePaneFocus);
  const value = useMemo<WorkspaceFocusContextValue>(
    () => ({
      workspaceKey,
      unfocusPane,
      restorePaneFocus,
    }),
    [restorePaneFocus, unfocusPane, workspaceKey],
  );

  return <WorkspaceFocusContext.Provider value={value}>{children}</WorkspaceFocusContext.Provider>;
}
