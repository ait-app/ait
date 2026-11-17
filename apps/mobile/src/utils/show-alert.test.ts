import { afterEach, describe, expect, it, vi } from "vitest";

type MockPlatform = "web" | "ios";

async function loadModuleForPlatform(platform: MockPlatform): Promise<{
  module: typeof import("./show-alert");
  nativeAlert: ReturnType<typeof vi.fn>;
}> {
  vi.resetModules();

  const nativeAlert = vi.fn();
  vi.doMock("react-native", () => ({
    Alert: { alert: nativeAlert },
    Platform: { OS: platform },
  }));
  vi.doMock("@/desktop/host", () => ({
    isElectronRuntime: () => false,
    isElectronRuntimeMac: () => false,
  }));

  return { module: await import("./show-alert"), nativeAlert };
}

describe("showAlert", () => {
  afterEach(() => {
    vi.doUnmock("react-native");
    vi.doUnmock("@/desktop/host");
    vi.restoreAllMocks();
    vi.resetModules();
    delete (globalThis as { alert?: unknown }).alert;
  });

  it("uses the native alert on iOS and Android", async () => {
    const { module, nativeAlert } = await loadModuleForPlatform("ios");
    const presenter = vi.fn();
    module.setAlertPresenter(presenter);

    module.showAlert("Unable to save", "The host is offline.");

    expect(nativeAlert).toHaveBeenCalledWith("Unable to save", "The host is offline.");
    expect(presenter).not.toHaveBeenCalled();
  });

  it("hands the notice to the registered presenter on web, where the native alert shows nothing", async () => {
    const { module, nativeAlert } = await loadModuleForPlatform("web");
    const presenter = vi.fn();
    module.setAlertPresenter(presenter);

    module.showAlert("Unable to save", "The host is offline.");

    expect(presenter).toHaveBeenCalledWith({
      title: "Unable to save",
      message: "The host is offline.",
    });
    expect(nativeAlert).not.toHaveBeenCalled();
  });

  it("falls back to the browser alert when no presenter is registered", async () => {
    const browserAlert = vi.fn();
    (globalThis as { alert?: unknown }).alert = browserAlert;
    const { module } = await loadModuleForPlatform("web");

    module.showAlert("Unable to save", "The host is offline.");

    expect(browserAlert).toHaveBeenCalledWith("Unable to save\nThe host is offline.");
  });

  it("stops using a presenter once it is removed, without dropping a newer one", async () => {
    const browserAlert = vi.fn();
    (globalThis as { alert?: unknown }).alert = browserAlert;
    const { module } = await loadModuleForPlatform("web");
    const first = vi.fn();
    const second = vi.fn();

    const removeFirst = module.setAlertPresenter(first);
    const removeSecond = module.setAlertPresenter(second);
    removeFirst();
    module.showAlert("Still shown");
    expect(second).toHaveBeenCalledWith({ title: "Still shown", message: undefined });

    removeSecond();
    module.showAlert("Browser only");
    expect(first).not.toHaveBeenCalled();
    expect(browserAlert).toHaveBeenCalledWith("Browser only");
  });
});
