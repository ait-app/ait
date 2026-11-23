// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { NativeHtmlExport } from "./native-html-export";

const mocks = vi.hoisted(() => ({
  guest: false,
  hostAvailable: true,
  provider: "omp",
  supported: true,
  start: vi.fn(),
  export: vi.fn(),
  cancel: vi.fn(),
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/runtime/host-runtime", () => {
  const host = () => ({ serverId: "host", guest: mocks.guest });
  const client = { exportAgentNativeHtml: mocks.export, cancelAgentNativeHtmlExport: mocks.cancel };
  return {
    getHostRuntimeStore: () => ({
      getHosts: () => (mocks.hostAvailable ? [host()] : []),
      getSnapshot: () => ({ connectionStatus: "online", connectionEpoch: 1, clientGeneration: 1 }),
      getClient: () => client,
    }),
    isSharedResourceHost: (candidate: { guest: boolean }) => candidate.guest,
    useHostRuntimeSnapshot: () => ({ connectionStatus: "online" }),
    useHosts: () => (mocks.hostAvailable ? [host()] : []),
  };
});
vi.mock("@/stores/session-store", () => {
  const state = () => ({
    sessions: {
      host: {
        serverInfo: { features: { agentNativeHtmlExport: mocks.supported } },
        agents: new Map([["agent", { provider: mocks.provider }]]),
      },
    },
  });
  const useSessionStore = (select: (value: ReturnType<typeof state>) => unknown) => select(state());
  useSessionStore.getState = state;
  return { useSessionStore };
});
vi.mock("@/stores/download-store", () => {
  const state = { downloads: new Map(), startNativeHtmlExport: mocks.start };
  const useDownloadStore = (select: (value: typeof state) => unknown) => select(state);
  useDownloadStore.getState = () => state;
  return { useDownloadStore };
});

beforeEach(() => {
  vi.clearAllMocks();
  mocks.guest = false;
  mocks.hostAvailable = true;
  mocks.provider = "omp";
  mocks.supported = true;
});
afterEach(cleanup);

it("offers the native download to the owner", () => {
  render(<NativeHtmlExport serverId="host" agentId="agent" />);
  fireEvent.click(screen.getByTestId("native-html-export-download"));
  expect(mocks.start).toHaveBeenCalledOnce();
});

it("hides native export for shared guest hosts", () => {
  mocks.guest = true;
  render(<NativeHtmlExport serverId="host" agentId="agent" />);
  expect(screen.queryByTestId("native-html-export")).toBeNull();
});

it("does not start if the host changed to a guest after render", () => {
  render(<NativeHtmlExport serverId="host" agentId="agent" />);
  mocks.guest = true;
  fireEvent.click(screen.getByTestId("native-html-export-download"));
  expect(mocks.start).not.toHaveBeenCalled();
});

it("does not offer a download while the host profile is unavailable", () => {
  mocks.hostAvailable = false;
  render(<NativeHtmlExport serverId="host" agentId="agent" />);
  expect(screen.queryByTestId("native-html-export-download")).toBeNull();
  expect(screen.getByText("nativeHtmlExport.unavailable")).toBeTruthy();
});

it("releases a generated artifact if the connection becomes a guest while waiting", async () => {
  const result = Promise.withResolvers<{ token: string }>();
  mocks.export.mockReturnValueOnce(result.promise);
  mocks.cancel.mockResolvedValueOnce({ released: true });
  render(<NativeHtmlExport serverId="host" agentId="agent" />);
  fireEvent.click(screen.getByTestId("native-html-export-download"));
  const options = mocks.start.mock.calls[0]![0] as { generate: () => Promise<unknown> };
  const pending = options.generate();
  mocks.guest = true;
  result.resolve({ token: "native-token" });
  await expect(pending).rejects.toThrow("nativeHtmlExport.unavailable");
  expect(mocks.cancel).toHaveBeenCalledExactlyOnceWith("agent", "native-token");
});
