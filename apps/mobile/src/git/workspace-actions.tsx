import { GitActionsSplitButton } from "@/git/actions-split-button";
import { useGitActions } from "@/git/use-actions";

interface WorkspaceActionsProps {
  serverId: string;
  cwd: string;
}

export function WorkspaceActions({ serverId, cwd }: WorkspaceActionsProps) {
  const { gitActions } = useGitActions({ serverId, cwd });

  return <GitActionsSplitButton gitActions={gitActions} />;
}
