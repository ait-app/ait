import type { HubLink } from "@ait/client/internal/hub";
import { getNativeHubLink, registerNativeAccountTransport } from "../native-account";
import { parseAccountRelayTarget } from "./account-relay-target";
import type { Transport, TransportFactory } from "./types";

/** Bound how long a new session waits for the account's Hub link. */
const HUB_READY_WAIT_MS = 5000;

interface Dependencies {
  /** The account's hub link; a promise while the account is still being restored. */
  hub(): HubLink | Promise<HubLink | null> | null;
  register(close: () => void): () => void;
}

function forward<T extends unknown[]>(handlers: Set<(...args: T) => void>) {
  return (...args: T) => {
    for (const handler of handlers) handler(...args);
  };
}

/**
 * Return a transport now and attach the one `choose` settles on; a session never switches.
 * The chosen transport is created and wired in one step so no early event is lost.
 */
function deferred(choose: () => Promise<() => Transport>): Transport {
  const opens = new Set<() => void>();
  const closes = new Set<(event?: unknown) => void>();
  const errors = new Set<(event?: unknown) => void>();
  const messages = new Set<(data: unknown, binary: boolean) => void>();
  let inner: Transport | undefined;
  let closed = false;
  choose()
    .then((create) => {
      if (closed) return;
      const transport = create();
      inner = transport;
      transport.onOpen(forward(opens));
      transport.onClose(forward(closes));
      transport.onError(forward(errors));
      transport.onMessage(forward(messages));
    })
    .catch((error: unknown) => {
      if (closed) return;
      closed = true;
      const failure = error instanceof Error ? error : new Error("Relay setup failed.");
      for (const handler of errors) handler(failure);
      for (const handler of closes) handler({ code: 1006, reason: failure.message });
    });
  return {
    recoverLiveness: () =>
      !closed && inner?.recoverLiveness ? inner.recoverLiveness() : Promise.resolve(false),
    send(data) {
      if (closed || !inner) throw new Error("Relay is not ready.");
      inner.send(data);
    },
    close(code, reason) {
      if (closed) return;
      closed = true;
      inner?.close(code, reason);
    },
    onOpen(handler) {
      opens.add(handler);
      return () => {
        opens.delete(handler);
      };
    },
    onClose(handler) {
      closes.add(handler);
      return () => {
        closes.delete(handler);
      };
    },
    onError(handler) {
      errors.add(handler);
      return () => {
        errors.delete(handler);
      };
    },
    onMessage(handler) {
      messages.add(handler);
      return () => {
        messages.delete(handler);
      };
    },
  };
}

/** A hub session closes with the account's other relay transports on sign-out or suspension. */
function hubTransport(link: HubLink, nodeId: string, deps: Dependencies): Transport {
  const session = link.openSession(nodeId);
  let unregister: () => void;
  try {
    unregister = deps.register(() => session.abort());
  } catch (error) {
    session.close();
    throw error;
  }
  session.onClose(() => unregister());
  return {
    recoverLiveness: () => session.recoverLiveness(),
    send: (data) => session.send(data),
    close() {
      unregister();
      session.close();
    },
    onOpen: (handler) => session.onOpen(handler),
    onClose: (handler) => session.onClose(handler),
    onError: (handler) => session.onError(handler),
    onMessage: (handler) => session.onMessage(handler),
  };
}

/**
 * All online business sessions require the account's Hub and the target Host's serve endpoint.
 * A failed setup reports an error; the client can retry after Hub presence recovers.
 */
export function createHubAccountRelayTransportFactory(
  deps: Dependencies = {
    hub: getNativeHubLink,
    register: registerNativeAccountTransport,
  },
): TransportFactory {
  return ({ url }) => {
    const target = parseAccountRelayTarget(url);
    const select = (link: HubLink | null): (() => Transport) => {
      if (!link?.ready || link.centerUrl !== target.center)
        throw new Error("Online Hub is unavailable.");
      const node = link.serveFor(target.hostId);
      if (!node) throw new Error("Target Host is unavailable on Hub.");
      return () => hubTransport(link, node.node_id, deps);
    };
    const hub = deps.hub();
    if (hub instanceof Promise || hub?.pending)
      return deferred(async () => {
        const link = await hub;
        if (link && !link.ready) await link.whenReady(HUB_READY_WAIT_MS);
        return select(link);
      });
    try {
      return select(hub)();
    } catch (error) {
      return deferred(() => Promise.reject(error));
    }
  };
}
