import {
  parseConnectionUri,
  serializeConnectionUri,
  serializeConnectionUriForStorage,
} from "@/utils/daemon-endpoints";
import { DaemonConnectionTestError } from "@/utils/test-daemon-connection";

export interface DirectConnectionDraft {
  host: string;
  port: string;
  useTls: boolean;
  password: string;
}

export interface PreparedDirectConnection {
  uri: string;
  endpoint: string;
  useTls: boolean;
  password?: string;
}

export interface DirectConnectionLabels {
  hostRequired: string;
  invalidPort: string;
  invalidConnection: string;
  failedToConnect: (endpoint: string) => string;
  noAdditionalDetails: (detail: string) => string;
  timedOut: string;
  refused: string;
  hostNotFound: string;
  hostUnreachable: string;
  tlsError: string;
  incorrectPassword: string;
  unableToConnect: string;
}

function isIpv6Host(host: string): boolean {
  return host.includes(":") && !host.startsWith("[") && !host.endsWith("]");
}

export function buildConnectionUriFromDraft(
  draft: DirectConnectionDraft,
  labels: DirectConnectionLabels,
): string {
  const host = draft.host.trim();
  const port = Number(draft.port.trim());
  if (!host) {
    throw new Error(labels.hostRequired);
  }
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(labels.invalidPort);
  }

  return serializeConnectionUriForStorage({
    host,
    port,
    isIpv6: isIpv6Host(host),
    useTls: draft.useTls,
    ...(draft.password ? { password: draft.password } : {}),
  });
}

export function prepareDirectConnection(
  draft: DirectConnectionDraft,
  labels: DirectConnectionLabels,
): PreparedDirectConnection {
  const parsed = parseConnectionUri(buildConnectionUriFromDraft(draft, labels));
  const endpoint = parsed.isIpv6
    ? `[${parsed.host}]:${parsed.port}`
    : `${parsed.host}:${parsed.port}`;

  return {
    uri: serializeConnectionUri(parsed),
    endpoint,
    useTls: parsed.useTls,
    ...(parsed.password ? { password: parsed.password } : {}),
  };
}

export function draftFromConnectionUri(uri: string): DirectConnectionDraft {
  const parsed = parseConnectionUri(uri);
  return {
    host: parsed.host,
    port: String(parsed.port),
    useTls: parsed.useTls,
    password: parsed.password ?? "",
  };
}

/**
 * Picks what Connect should use. The connection URI field sits below the host, port and password
 * fields, and until now it was only read when the Advanced section was collapsed. A URI the user
 * typed there wins over the fields; an untouched one (still the text generated when the section
 * opened) leaves the fields in charge, so editing them with the section open keeps working.
 */
export function resolveDirectConnectionDraft(
  input: {
    fields: DirectConnectionDraft;
    isAdvancedOpen: boolean;
    advancedUri: string;
    advancedUriWhenOpened: string;
  },
  labels: DirectConnectionLabels,
): DirectConnectionDraft {
  if (!input.isAdvancedOpen || !isConnectionUriEdited(input)) {
    return input.fields;
  }
  try {
    return draftFromConnectionUri(input.advancedUri);
  } catch {
    throw new Error(labels.invalidConnection);
  }
}

/** True when the URI field holds something other than the text generated when it opened. */
export function isConnectionUriEdited(input: {
  advancedUri: string;
  advancedUriWhenOpened: string;
}): boolean {
  const typedUri = input.advancedUri.trim();
  return typedUri.length > 0 && typedUri !== input.advancedUriWhenOpened.trim();
}

function normalizeTransportMessage(message: string | null | undefined): string | null {
  if (!message) return null;
  const trimmed = message.trim();
  if (!trimmed) return null;
  return trimmed;
}

function formatTechnicalTransportDetails(
  details: (string | null)[],
  labels: DirectConnectionLabels,
): string | null {
  const unique = Array.from(
    new Set(
      details
        .map((value) => normalizeTransportMessage(value))
        .filter((value): value is string => Boolean(value))
        .map((value) => value.trim())
        .filter((value) => value.length > 0),
    ),
  );

  if (unique.length === 0) return null;

  const allGeneric = unique.every((value) => {
    const lower = value.toLowerCase();
    return lower === "transport error" || lower === "transport closed";
  });

  if (allGeneric) {
    return labels.noAdditionalDetails(unique[0] ?? "");
  }

  return unique.join(" — ");
}

export function buildConnectionFailureCopy(input: {
  endpoint: string;
  error: unknown;
  labels: DirectConnectionLabels;
}): { title: string; detail: string | null; raw: string | null } {
  const { endpoint, error, labels } = input;
  const title = labels.failedToConnect(endpoint);

  const raw = (() => {
    if (error instanceof DaemonConnectionTestError) {
      return (
        formatTechnicalTransportDetails([error.reason, error.lastError], labels) ??
        normalizeTransportMessage(error.message)
      );
    }
    if (error instanceof Error) {
      return normalizeTransportMessage(error.message);
    }
    return null;
  })();

  const rawLower = raw?.toLowerCase() ?? "";
  let detail: string | null = null;

  if (error instanceof DaemonConnectionTestError && error.incorrectPassword) {
    // The probe has already classified the failure. `raw` holds the transport text ("code 4001"),
    // which says nothing about the password.
    detail = labels.incorrectPassword;
  } else if (rawLower.includes("timed out")) {
    detail = labels.timedOut;
  } else if (
    rawLower.includes("econnrefused") ||
    rawLower.includes("connection refused") ||
    rawLower.includes("err_connection_refused")
  ) {
    detail = labels.refused;
  } else if (rawLower.includes("enotfound") || rawLower.includes("not found")) {
    detail = labels.hostNotFound;
  } else if (rawLower.includes("ehostunreach") || rawLower.includes("host is unreachable")) {
    detail = labels.hostUnreachable;
  } else if (
    rawLower.includes("certificate") ||
    rawLower.includes("tls") ||
    rawLower.includes("ssl")
  ) {
    detail = labels.tlsError;
  } else {
    detail = labels.unableToConnect;
  }

  return { title, detail, raw };
}
