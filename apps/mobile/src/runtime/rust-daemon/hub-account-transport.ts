import type { HubLink } from "@ait/client/internal/hub";
import { getNativeHubLink, registerNativeAccountTransport } from "../native-account";
import { parseAccountRelayTarget } from "./account-relay-target";
import { createNativeAccountRelayTransportFactory } from "./native-account-transport";
import type { Transport, TransportFactory } from "./types";

/** Why a new account relay session took its path; kept for retiring the legacy relay. */
export type AccountRelayPath = "hub" | "legacy:no_serve" | "legacy:hub_down";

/** A connecting hub link may delay a new session this long before it falls back to legacy. */
const HUB_READY_WAIT_MS = 5000;

interface Dependencies {
  /** The account's hub link; a promise while the account is still being restored. */
  hub(): HubLink | Promise<HubLink | null> | null;
  legacy: TransportFactory;
  register(close: () => void): () => void;
  log(path: AccountRelayPath, fields: object): void;
}

function choose(
  link: HubLink | null,
  target: { hostId: string; center: string },
): { path: AccountRelayPath; nodeId?: string } {
  if (!link?.ready || link.centerUrl !== target.center) return { path: "legacy:hub_down" };
  const node = link.serveFor(target.hostId);
  return node ? { path: "hub", nodeId: node.node_id } : { path: "legacy:no_serve" };
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
 * Native account relays use the hub when presence lists the target Host's serve endpoint, and
 * the unchanged legacy relay otherwise. The choice is made once, when the session starts.
 */
export function createHubAccountRelayTransportFactory(
  deps: Dependencies = {
    hub: getNativeHubLink,
    legacy: createNativeAccountRelayTransportFactory(),
    register: registerNativeAccountTransport,
    log: (path, fields) => console.info("[AccountRelay] transport path", { path, ...fields }),
  },
): TransportFactory {
  return ({ url }) => {
    let target: { hostId: string; center: string };
    try {
      target = parseAccountRelayTarget(url);
    } catch {
      // The legacy transport reports invalid targets exactly as before.
      return deps.legacy({ url });
    }
    const select = (link: HubLink | null): (() => Transport) => {
      const { path, nodeId } = choose(link, target);
      deps.log(path, { hostId: target.hostId, nodeId, hub: link?.state ?? "absent" });
      return path === "hub" && link && nodeId
        ? () => hubTransport(link, nodeId, deps)
        : () => deps.legacy({ url });
    };
    const hub = deps.hub();
    if (hub instanceof Promise || hub?.pending)
      return deferred(async () => {
        const link = await hub;
        if (link && !link.ready) await link.whenReady(HUB_READY_WAIT_MS);
        return select(link);
      });
    const create = select(hub);
    try {
      return create();
    } catch (error) {
      return deferred(() => Promise.reject(error));
    }
  };
}
