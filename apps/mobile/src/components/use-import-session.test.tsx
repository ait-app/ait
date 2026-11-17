/** @vitest-environment jsdom */
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNavigateToImportedAgent } from "./use-import-session";

const mocks = vi.hoisted(() => ({
  push: vi.fn(),
  openProject: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("expo-router", () => ({ useRouter: () => ({ push: mocks.push }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/components/import-session-sheet", () => ({ ImportSessionSheet: () => null }));
vi.mock("@/hosts/host-chooser", () => ({ useHostChooser: () => vi.fn() }));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => null }));
vi.mock("@/hooks/use-open-project", () => ({ useOpenProject: () => mocks.openProject }));
vi.mock("@/contexts/toast-context", () => ({
  useToast: () => ({ show: vi.fn(), copied: vi.fn(), error: mocks.toastError }),
}));

const agent = { id: "agent-1", cwd: "/repo/other" };

describe("useNavigateToImportedAgent", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it("opens the imported session once its project is registered", async () => {
    mocks.openProject.mockResolvedValue({ ok: true, project: {} });
    const { result } = renderHook(() => useNavigateToImportedAgent("host-1"));

    await result.current(agent);

    expect(mocks.openProject).toHaveBeenCalledWith("/repo/other");
    expect(mocks.push).toHaveBeenCalledTimes(1);
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("tells the user when the project cannot be opened instead of doing nothing", async () => {
    mocks.openProject.mockResolvedValue({ ok: false, errorCode: null, error: "unsupported" });
    const { result } = renderHook(() => useNavigateToImportedAgent("host-1"));

    await result.current(agent);

    expect(mocks.push).not.toHaveBeenCalled();
    expect(mocks.toastError).toHaveBeenCalledWith("importSession.status.openImportedFailed");
  });

  it("reports a rejected add-project request rather than leaving it unhandled", async () => {
    mocks.openProject.mockRejectedValue(new Error("Transport closed"));
    const { result } = renderHook(() => useNavigateToImportedAgent("host-1"));

    await expect(result.current(agent)).resolves.toBeUndefined();

    expect(mocks.push).not.toHaveBeenCalled();
    expect(mocks.toastError).toHaveBeenCalledWith("importSession.status.openImportedFailed");
  });
});
