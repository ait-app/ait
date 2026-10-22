import type { MethodSpec } from "./methods";

// Transport-owned additions to the canonical Ait method catalog.
export const HUB_METHODS: Readonly<Record<string, MethodSpec>> = {
  "hub.status.request": {
    method: "hub.status.request",
    kind: "request",
    channel: 1,
    response: "hub.status.response",
  },
  "hub.start.request": {
    method: "hub.start.request",
    kind: "request",
    channel: 1,
    response: "hub.start.response",
  },
  "hub.stop.request": {
    method: "hub.stop.request",
    kind: "request",
    channel: 1,
    response: "hub.stop.response",
  },
};
