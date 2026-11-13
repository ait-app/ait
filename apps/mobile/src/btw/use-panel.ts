import { useCallback, useEffect, useRef, useState } from "react";

export interface BtwPanelRequest {
  owner: string;
  question: string;
  key: number;
}

interface BtwPanelState {
  owner: string;
  opened: boolean;
  request: BtwPanelRequest | null;
}

/** Own pending commands above the floating panel's fullscreen/input-mode lifetime. */
export function useBtwPanelState(input: { serverId: string; agentId: string }) {
  const owner = `${input.serverId}:${input.agentId}`;
  const [state, setState] = useState<BtwPanelState>({ owner, opened: false, request: null });
  const nextKey = useRef(0);

  useEffect(() => {
    setState((current) =>
      current.owner === owner ? current : { owner, opened: false, request: null },
    );
  }, [owner]);

  const open = useCallback(
    (question: string) => {
      nextKey.current += 1;
      setState({
        owner,
        opened: true,
        request: question ? { owner, question, key: nextKey.current } : null,
      });
    },
    [owner],
  );

  const consume = useCallback(
    (key: number) => {
      setState((current) =>
        current.owner === owner && current.request?.key === key
          ? { ...current, request: null }
          : current,
      );
    },
    [owner],
  );

  const onOpenedChange = useCallback(
    (opened: boolean) => {
      setState((current) => (current.owner === owner ? { ...current, opened } : current));
    },
    [owner],
  );

  return {
    opened: state.owner === owner && state.opened,
    request: state.owner === owner ? state.request : null,
    open,
    consume,
    onOpenedChange,
  };
}
