type TransportUrl = Pick<
  URL,
  | "protocol"
  | "hostname"
  | "pathname"
  | "port"
  | "username"
  | "password"
  | "search"
  | "searchParams"
  | "hash"
>;

/** Internal targets must not depend on a browser's non-special URL authority parsing. */
export function parseRustDaemonTransportUrl(url: string): TransportUrl {
  if (!/^ait\+desktop:/i.test(url)) return new URL(url);

  // Match the whole authority, not a prefix: credentials, ports and lookalike hosts
  // are not valid internal targets. Keep the raw path so it cannot be normalized
  // into a valid host ID or SSH endpoint by a URL implementation.
  const match = /^ait\+desktop:\/\/(account-relay|ssh)(\/[^?#]*)?(\?[^#]*)?(#.*)?$/i.exec(url);
  if (!match || /[\s\\]/.test(url)) {
    throw new Error("Invalid Rust daemon WebSocket endpoint");
  }
  const search = match[3] ?? "";
  return {
    protocol: "ait+desktop:",
    hostname: match[1],
    pathname: match[2] ?? "",
    port: "",
    username: "",
    password: "",
    search,
    searchParams: new URLSearchParams(search),
    hash: match[4] ?? "",
  };
}
