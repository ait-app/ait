import { describe, expect, it } from "vitest";
import { DaemonConnectionTestError } from "@/utils/test-daemon-connection";
import {
  buildConnectionFailureCopy,
  prepareDirectConnection,
  resolveDirectConnectionDraft,
  type DirectConnectionDraft,
  type DirectConnectionLabels,
} from "./add-host-direct-connection";

const labels: DirectConnectionLabels = {
  hostRequired: "host required",
  invalidPort: "invalid port",
  invalidConnection: "invalid connection",
  failedToConnect: (endpoint) => `failed to connect to ${endpoint}`,
  noAdditionalDetails: (detail) => `${detail} (no details)`,
  timedOut: "timed out",
  refused: "refused",
  hostNotFound: "host not found",
  hostUnreachable: "host unreachable",
  tlsError: "tls error",
  incorrectPassword: "incorrect password",
  unableToConnect: "unable to connect",
};

const defaultFields: DirectConnectionDraft = {
  host: "127.0.0.1",
  port: "7316",
  useTls: false,
  password: "",
};

describe("resolveDirectConnectionDraft", () => {
  it("uses the fields while the Advanced section is closed", () => {
    expect(
      resolveDirectConnectionDraft(
        {
          fields: defaultFields,
          isAdvancedOpen: false,
          advancedUri: "tcp://10.0.0.5:7443",
          advancedUriWhenOpened: "",
        },
        labels,
      ),
    ).toBe(defaultFields);
  });

  it("connects to the URI typed in the Advanced section instead of the default fields", () => {
    const draft = resolveDirectConnectionDraft(
      {
        fields: defaultFields,
        isAdvancedOpen: true,
        advancedUri: " tcp://10.0.0.5:7443?ssl=true&password=secret ",
        advancedUriWhenOpened: "tcp://127.0.0.1:7316",
      },
      labels,
    );

    expect(draft).toEqual({ host: "10.0.0.5", port: "7443", useTls: true, password: "secret" });
    expect(prepareDirectConnection(draft, labels)).toMatchObject({
      endpoint: "10.0.0.5:7443",
      useTls: true,
      password: "secret",
    });
  });

  it("keeps the fields in charge when the generated URI was left untouched", () => {
    const editedFields = { ...defaultFields, host: "devbox.local" };

    expect(
      resolveDirectConnectionDraft(
        {
          fields: editedFields,
          isAdvancedOpen: true,
          advancedUri: "tcp://127.0.0.1:7316",
          advancedUriWhenOpened: "tcp://127.0.0.1:7316",
        },
        labels,
      ),
    ).toBe(editedFields);
  });

  it("keeps the fields in charge when the URI field was emptied", () => {
    expect(
      resolveDirectConnectionDraft(
        {
          fields: defaultFields,
          isAdvancedOpen: true,
          advancedUri: "   ",
          advancedUriWhenOpened: "tcp://127.0.0.1:7316",
        },
        labels,
      ),
    ).toBe(defaultFields);
  });

  it("rejects a typed URI that cannot be parsed rather than falling back to the fields", () => {
    expect(() =>
      resolveDirectConnectionDraft(
        {
          fields: defaultFields,
          isAdvancedOpen: true,
          advancedUri: "10.0.0.5:7443",
          advancedUriWhenOpened: "tcp://127.0.0.1:7316",
        },
        labels,
      ),
    ).toThrow("invalid connection");
  });
});

describe("buildConnectionFailureCopy", () => {
  it("names a rejected password instead of a generic connection failure", () => {
    const error = new DaemonConnectionTestError("Incorrect password", {
      reason: "Transport closed (code 4001)",
      lastError: "Transport closed (code 4001)",
      incorrectPassword: true,
    });

    expect(buildConnectionFailureCopy({ endpoint: "tcp://lan:7316", error, labels })).toEqual({
      title: "failed to connect to tcp://lan:7316",
      detail: "incorrect password",
      raw: "Transport closed (code 4001)",
    });
  });

  it("still reports an unclassified transport failure as unable to connect", () => {
    const error = new DaemonConnectionTestError("Transport closed (code 4001)", {
      reason: "Transport closed (code 4001)",
      lastError: null,
    });

    expect(buildConnectionFailureCopy({ endpoint: "tcp://lan:7316", error, labels }).detail).toBe(
      "unable to connect",
    );
  });

  it("maps a refused connection to its own message", () => {
    const error = new DaemonConnectionTestError("connect ECONNREFUSED 10.0.0.5:7316", {
      reason: "connect ECONNREFUSED 10.0.0.5:7316",
      lastError: null,
    });

    expect(buildConnectionFailureCopy({ endpoint: "tcp://lan:7316", error, labels }).detail).toBe(
      "refused",
    );
  });
});
