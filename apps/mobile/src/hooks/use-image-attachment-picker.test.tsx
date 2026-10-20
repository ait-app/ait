/** @vitest-environment jsdom */
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  PermissionStatus,
  type CameraPermissionResponse,
  type ImagePickerResult,
} from "expo-image-picker";
import { useImageAttachmentPicker } from "./use-image-attachment-picker";
import { appErrorReporter } from "@/diagnostics/error-reporter";

const mocks = vi.hoisted(() => ({
  native: true,
  electron: false,
  alert: vi.fn(),
  cameraPermission: vi.fn<() => Promise<CameraPermissionResponse>>(),
  requestCameraPermission: vi.fn<() => Promise<CameraPermissionResponse>>(),
  requestMediaPermission: vi.fn(),
  pendingResult: vi.fn<() => Promise<ImagePickerResult | null>>(),
  launchCamera: vi.fn<() => Promise<ImagePickerResult>>(),
  launchLibrary: vi.fn<() => Promise<ImagePickerResult>>(),
  desktopDialog: vi.fn(),
  exportAsPng: vi.fn<() => Promise<string>>(),
}));

vi.mock("react-native", () => ({ Alert: { alert: mocks.alert } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/constants/platform", () => ({
  get isNative() {
    return mocks.native;
  },
  get isWeb() {
    return !mocks.native;
  },
}));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => ({ dialog: {} }),
  isElectronRuntime: () => mocks.electron,
}));
vi.mock("expo-image-picker", () => ({
  PermissionStatus: { UNDETERMINED: "undetermined", GRANTED: "granted", DENIED: "denied" },
  useMediaLibraryPermissions: () => [null, mocks.requestMediaPermission],
  getCameraPermissionsAsync: mocks.cameraPermission,
  requestCameraPermissionsAsync: mocks.requestCameraPermission,
  getPendingResultAsync: mocks.pendingResult,
  launchCameraAsync: mocks.launchCamera,
  launchImageLibraryAsync: mocks.launchLibrary,
}));
vi.mock("@/hooks/image-attachment-picker", async () => {
  const { normalizePickedImageAssetsWith } = await import("./picked-image-normalizer");
  return {
    normalizePickedImageAssets: (assets: Parameters<typeof normalizePickedImageAssetsWith>[0]) =>
      normalizePickedImageAssetsWith(assets, mocks.exportAsPng),
    pickImagesWithDesktopDialog: mocks.desktopDialog,
  };
});

function permission(granted: boolean, canAskAgain = true): CameraPermissionResponse {
  return {
    granted,
    canAskAgain,
    expires: "never",
    status: granted ? PermissionStatus.GRANTED : PermissionStatus.DENIED,
  };
}

const photo: ImagePickerResult = {
  canceled: false,
  assets: [{ uri: "file:///cache/camera.jpg", mimeType: "image/jpeg", width: 800, height: 600 }],
};

beforeEach(() => {
  appErrorReporter.takePending();
  vi.resetAllMocks();
  mocks.native = true;
  mocks.electron = false;
  mocks.cameraPermission.mockResolvedValue(permission(true));
  mocks.requestCameraPermission.mockResolvedValue(permission(true));
  mocks.requestMediaPermission.mockResolvedValue(permission(true));
  mocks.pendingResult.mockResolvedValue(null);
  mocks.launchCamera.mockResolvedValue(photo);
  mocks.launchLibrary.mockResolvedValue(photo);
  mocks.exportAsPng.mockResolvedValue("file:///cache/converted.png");
});

afterEach(cleanup);

describe("camera attachments", () => {
  it("requests camera permission and returns a photo without requesting photo library access", async () => {
    mocks.cameraPermission.mockResolvedValue(permission(false));
    const { result } = renderHook(useImageAttachmentPicker);

    await expect(result.current.takePhoto()).resolves.toEqual([
      {
        source: { kind: "file_uri", uri: "file:///cache/camera.jpg" },
        mimeType: "image/jpeg",
        fileName: null,
      },
    ]);
    expect(mocks.requestCameraPermission).toHaveBeenCalledOnce();
    expect(mocks.launchCamera).toHaveBeenCalledWith({ mediaTypes: ["images"], quality: 0.8 });
    expect(mocks.requestMediaPermission).not.toHaveBeenCalled();
    expect(mocks.launchLibrary).not.toHaveBeenCalled();
  });

  it.each([true, false])("handles denied camera access (canAskAgain: %s)", async (canAskAgain) => {
    mocks.cameraPermission.mockResolvedValue(permission(false, canAskAgain));
    mocks.requestCameraPermission.mockResolvedValue(permission(false, canAskAgain));
    const { result } = renderHook(useImageAttachmentPicker);

    await expect(result.current.takePhoto()).resolves.toBeNull();
    expect(mocks.requestCameraPermission).toHaveBeenCalledTimes(canAskAgain ? 1 : 0);
    expect(mocks.launchCamera).not.toHaveBeenCalled();
    expect(mocks.alert).toHaveBeenCalledWith(
      "imageAttachmentPicker.permissionTitle",
      "imageAttachmentPicker.cameraPermissionMessage",
    );

    mocks.cameraPermission.mockResolvedValue(permission(true));
    await expect(result.current.takePhoto()).resolves.toHaveLength(1);
  });

  it("returns no attachments on cancellation and allows the next capture", async () => {
    mocks.launchCamera.mockResolvedValueOnce({ canceled: true, assets: null });
    const { result } = renderHook(useImageAttachmentPicker);

    await expect(result.current.takePhoto()).resolves.toBeNull();
    expect(mocks.alert).not.toHaveBeenCalled();
    await expect(result.current.takePhoto()).resolves.toHaveLength(1);
  });

  it("normalizes HEIC camera photos through the native image conversion", async () => {
    mocks.launchCamera.mockResolvedValue({
      canceled: false,
      assets: [
        {
          uri: "file:///cache/camera.heic",
          fileName: "IMG.HEIC",
          mimeType: "image/heic",
          width: 800,
          height: 600,
        },
      ],
    });
    const { result } = renderHook(useImageAttachmentPicker);

    await expect(result.current.takePhoto()).resolves.toEqual([
      {
        source: { kind: "file_uri", uri: "file:///cache/converted.png" },
        mimeType: "image/png",
        fileName: "IMG.png",
      },
    ]);
    expect(mocks.exportAsPng).toHaveBeenCalledWith("file:///cache/camera.heic");
  });

  it("recovers an Android pending photo before opening another camera", async () => {
    mocks.pendingResult.mockResolvedValue(photo);
    const { result } = renderHook(useImageAttachmentPicker);

    await expect(result.current.takePhoto()).resolves.toHaveLength(1);
    expect(mocks.launchCamera).not.toHaveBeenCalled();
  });

  it("prevents overlapping camera and library pickers", async () => {
    let finishCapture!: (result: ImagePickerResult) => void;
    mocks.launchCamera.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishCapture = resolve;
        }),
    );
    const { result } = renderHook(useImageAttachmentPicker);
    const capture = result.current.takePhoto();
    await vi.waitFor(() => expect(mocks.launchCamera).toHaveBeenCalledOnce());

    await expect(result.current.takePhoto()).resolves.toBeNull();
    await expect(result.current.pickImages()).resolves.toBeNull();
    expect(mocks.launchLibrary).not.toHaveBeenCalled();
    finishCapture(photo);
    await capture;
    await expect(result.current.pickImages()).resolves.toHaveLength(1);
  });

  it("reports a capture failure and releases the picker for retry", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.launchCamera.mockRejectedValueOnce(new Error("Camera unavailable"));
    const { result } = renderHook(useImageAttachmentPicker);
    try {
      await expect(result.current.takePhoto()).resolves.toBeNull();
      const [report] = appErrorReporter.takePending();
      expect(report.message).toContain("imageAttachmentPicker.failedToTakePhoto");
      expect(report.details).toContain("Camera unavailable");
      expect(mocks.alert).not.toHaveBeenCalled();
      await expect(result.current.takePhoto()).resolves.toHaveLength(1);
    } finally {
      consoleError.mockRestore();
    }
  });

  it("leaves Electron image selection on the desktop dialog", async () => {
    mocks.native = false;
    mocks.electron = true;
    mocks.desktopDialog.mockResolvedValue([
      { source: { kind: "file_uri", uri: "/photo.png" }, mimeType: "image/png" },
    ]);
    const { result } = renderHook(useImageAttachmentPicker);

    await expect(result.current.takePhoto()).resolves.toBeNull();
    await expect(result.current.pickImages()).resolves.toHaveLength(1);
    expect(mocks.desktopDialog).toHaveBeenCalledOnce();
    expect(mocks.cameraPermission).not.toHaveBeenCalled();
    expect(mocks.launchLibrary).not.toHaveBeenCalled();
  });
});
