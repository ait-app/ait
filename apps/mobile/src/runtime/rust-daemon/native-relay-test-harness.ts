import { vi } from "vitest";
import type { Transport } from "./types";

export function nativeRelayHarness() {
  const callbacks = {
    message: (_data: unknown, _binary: boolean) => {},
    close: () => {},
    error: () => {},
  };
  const socket: Transport = {
    send: vi.fn(),
    close: vi.fn(),
    onOpen: () => () => {},
    onMessage: (handler) => {
      callbacks.message = handler;
      return () => {
        callbacks.message = () => {};
      };
    },
    onClose: (handler) => {
      callbacks.close = handler;
      return () => {
        callbacks.close = () => {};
      };
    },
    onError: (handler) => {
      callbacks.error = handler;
      return () => {
        callbacks.error = () => {};
      };
    },
  };
  const grant = {
    relay_session_id: "visit",
    server_id: "server",
    instance_id: "instance",
    client_ticket: "private-ticket",
    url: "wss://example.test/api/v1/relay/sessions/visit/client",
  };
  const account = {
    openVisit: vi.fn(async () => grant),
    openDownload: vi.fn(async () => grant),
    closeVisit: vi.fn(async () => {}),
  };
  let cancel = () => {};
  const remove = vi.fn();
  const deps = {
    account: async () => account,
    connect: vi.fn(() => socket),
    register: vi.fn((close: () => void) => {
      cancel = close;
      return remove;
    }),
  };
  return {
    deps,
    account,
    grant,
    socket,
    remove,
    cancel: () => cancel(),
    message: (data: unknown, binary = false) =>
      callbacks.message(binary ? data : JSON.stringify(data), binary),
    disconnect: () => callbacks.close(),
    flush: async () => {
      for (let i = 0; i < 12; i++) await Promise.resolve();
    },
  };
}
