import type { MethodSpec } from "./methods";

// Transport-owned additions to the pinned Paseo method catalog.
export const RELAY_METHODS: Readonly<Record<string, MethodSpec>> = {
  "relay.status.request": {
    method: "relay.status.request",
    kind: "request",
    channel: 1,
    response: "relay.status.response",
  },
  "relay.start.request": {
    method: "relay.start.request",
    kind: "request",
    channel: 1,
    response: "relay.start.response",
  },
  "relay.stop.request": {
    method: "relay.stop.request",
    kind: "request",
    channel: 1,
    response: "relay.stop.response",
  },
};
