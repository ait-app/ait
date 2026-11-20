import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { CheckoutStatusPayload } from "@/git/checkout-status-cache";
import type { ForgeRef } from "@/git/forge-refs";
import { checkoutStatusQueryKey } from "@/git/query-keys";
import { resolveForgeLinkRef } from "./model";

export interface ForgeLinkTarget {
  serverId: string;
  cwd: string;
  forgeRef: ForgeRef;
}

interface ForgeLinkWorkspace {
  serverId: string | undefined;
  cwd: string | undefined;
}

export type ResolveForgeLinkTarget = (
  href: string,
  workspace: ForgeLinkWorkspace,
) => ForgeLinkTarget | null;

/**
 * Resolves a link against the checkout status the workspace already holds.
 *
 * It reads the cache rather than subscribing because a message can carry dozens of links and the
 * answer is only needed for the one being pointed at. A workspace whose status has not arrived
 * resolves nothing, which leaves the link an ordinary link.
 */
export function useResolveForgeLinkTarget(): ResolveForgeLinkTarget {
  const queryClient = useQueryClient();
  return useCallback(
    (href, { serverId, cwd }) => {
      if (!serverId || !cwd) return null;
      const status = queryClient.getQueryData<CheckoutStatusPayload>(
        checkoutStatusQueryKey(serverId, cwd),
      );
      const remoteUrl = status?.isGit ? status.remoteUrl : null;
      const forgeRef = resolveForgeLinkRef(href, remoteUrl);
      return forgeRef ? { serverId, cwd, forgeRef } : null;
    },
    [queryClient],
  );
}
