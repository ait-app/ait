// macOS launches these bundle directories instead of browsing them when they are opened.
const MAC_LAUNCHABLE_BUNDLE = /\.(app|pkg|mpkg|workflow|action|prefpane|saver)$/i;

/** Whether opening a directory would run code instead of showing its contents. */
export function isLaunchableBundle(directory: string, platform: NodeJS.Platform): boolean {
  return platform === "darwin" && MAC_LAUNCHABLE_BUNDLE.test(directory.replace(/\/+$/, ""));
}
