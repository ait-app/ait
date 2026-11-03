import { normalizeCenter } from "@ait/client/internal/account-session";
import { parseRustDaemonTransportUrl } from "./transport-url";

/** Resolve a saved host's non-secret service binding before requesting a relay ticket. */
export function parseAccountRelayTarget(url: string): {
  hostId: string;
  center: string;
  shareId?: string;
  nodeId?: string;
} {
  const target = parseRustDaemonTransportUrl(url);
  const centers = target.searchParams.getAll("center");
  const shares = target.searchParams.getAll("share");
  const nodes = target.searchParams.getAll("node");
  if (
    target.protocol !== "ait+desktop:" ||
    target.hostname !== "account-relay" ||
    !/^\/[0-9a-f-]{36}$/i.test(target.pathname) ||
    target.port ||
    target.hash ||
    target.username ||
    target.password ||
    centers.length !== 1 ||
    !centers[0]?.trim() ||
    [...target.searchParams.keys()].some((key) => !["center", "share", "node"].includes(key)) ||
    shares.length > 1 ||
    nodes.length > 1 ||
    (shares.length === 0) !== (nodes.length === 0) ||
    (shares[0] !== undefined && !/^[0-9a-f-]{36}$/i.test(shares[0])) ||
    (nodes[0] !== undefined && !/^[A-Za-z0-9._:-]{1,200}$/.test(nodes[0]))
  ) {
    throw new Error("Invalid account relay target.");
  }
  return {
    hostId: target.pathname.slice(1),
    center: normalizeCenter(centers[0]),
    ...(shares[0] && nodes[0] ? { shareId: shares[0], nodeId: nodes[0] } : {}),
  };
}
