import { Platform } from "react-native";

/** Export the already sanitized report as an attachment, keeping message bodies short. */
export async function exportDiagnosticReport(report: string): Promise<void> {
  const filename = `ait-diagnostics-${new Date().toISOString().replace(/[:.]/g, "-")}.txt`;
  if (Platform.OS === "web") {
    const url = URL.createObjectURL(new Blob([report], { type: "text/plain;charset=utf-8" }));
    const link = document.createElement("a");
    try {
      link.href = url;
      link.download = filename;
      document.body.appendChild(link);
      link.click();
    } finally {
      link.remove();
      // Give the browser / Electron download handler time to consume the URL.
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    }
    return;
  }
  const FileSystem = await import("expo-file-system/legacy");
  const Sharing = await import("expo-sharing");
  if (!FileSystem.cacheDirectory || !(await Sharing.isAvailableAsync())) {
    throw new Error("File sharing is unavailable");
  }
  const uri = `${FileSystem.cacheDirectory}${filename}`;
  try {
    await FileSystem.writeAsStringAsync(uri, report, { encoding: FileSystem.EncodingType.UTF8 });
    await Sharing.shareAsync(uri, { mimeType: "text/plain", UTI: "public.plain-text" });
  } finally {
    await FileSystem.deleteAsync(uri, { idempotent: true });
  }
}
