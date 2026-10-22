const { getDefaultConfig } = require("expo/metro-config");
const { resolve } = require("metro-resolver");
const fs = require("fs");
const path = require("path");
const { createHash } = require("node:crypto");

const projectRoot = __dirname;
const reactRoot = path.dirname(require.resolve("react/package.json"));
const reactDomRoot = path.dirname(require.resolve("react-dom/package.json"));
const appSrcRoot = path.resolve(projectRoot, "src");
const isFdroidBuild = process.env.PASEO_FDROID_BUILD === "1";
const fdroidModuleOverrides = {
  "expo-notifications": path.resolve(appSrcRoot, "fdroid/expo-notifications.ts"),
};
const customWebPlatform = (process.env.AIT_WEB_PLATFORM ?? "")
  .trim()
  .replace(/^\./, "")
  .toLowerCase();

const config = getDefaultConfig(projectRoot);
// Shared brand geometry lives outside the workspace packages watched by Expo.
config.watchFolders = [...(config.watchFolders ?? []), path.resolve(projectRoot, "../../assets")];
// Expo inlines its public manifest into expo-constants. Those dependency files
// do not change when release metadata does, so invalidate their cached transform.
const manifestHash = createHash("sha256");
for (const file of [
  "app.config.js",
  "package.json",
  "../../CHANGELOG.md",
  "../../assets/brand/ait-mark.json",
]) {
  manifestHash.update(fs.readFileSync(path.resolve(projectRoot, file)));
}
config.cacheVersion = `${config.cacheVersion ?? ""}:${manifestHash.digest("hex")}`;
const defaultResolveRequest = config.resolver.resolveRequest ?? resolve;

// Keep app exports deterministic across dev machines and CI. Metro's Watchman
// crawler behavior depends on the host Watchman build/capabilities, while the
// node crawler is the path used when Watchman is absent.
config.resolver.useWatchman = false;

const escapedAppSrcRoot = appSrcRoot
  .split(path.sep)
  .map((segment) => segment.replace(/[|\\{}()[\]^$+*?.]/g, "\\$&"))
  .join("[\\\\/]");
const pathSeparatorPattern = "[\\\\/]";

config.resolver.extraNodeModules = {
  ...config.resolver.extraNodeModules,
  react: reactRoot,
  "react-dom": reactDomRoot,
  "react/jsx-runtime": path.join(reactRoot, "jsx-runtime"),
  "react/jsx-dev-runtime": path.join(reactRoot, "jsx-dev-runtime"),
};
config.resolver.blockList = new RegExp(
  `(^${escapedAppSrcRoot}${pathSeparatorPattern}.*\\.(test|spec)\\.(ts|tsx)$|${pathSeparatorPattern}__tests__${pathSeparatorPattern}.*)$`,
);

function isLocalModuleImport(moduleName) {
  return (
    moduleName.startsWith("./") ||
    moduleName.startsWith("../") ||
    moduleName.startsWith("@/") ||
    path.isAbsolute(moduleName)
  );
}

function resolveWithCustomWebOverlay(context, moduleName, platform) {
  const shouldResolveCustomWebVariant =
    platform === "web" &&
    customWebPlatform.length > 0 &&
    customWebPlatform !== "web" &&
    isLocalModuleImport(moduleName);

  if (shouldResolveCustomWebVariant) {
    const overlayContext = {
      ...context,
      // Resolve only "<custom-platform>.<ext>" variants in overlay mode.
      sourceExts: context.sourceExts.map((ext) => `${customWebPlatform}.${ext}`),
      preferNativePlatform: false,
    };

    try {
      return defaultResolveRequest(overlayContext, moduleName, null);
    } catch {
      // Ignore overlay misses and continue with normal web resolution.
    }
  }

  return defaultResolveRequest(context, moduleName, platform);
}

config.resolver.resolveRequest = (context, moduleName, platform) => {
  if (isFdroidBuild && platform === "android" && fdroidModuleOverrides[moduleName]) {
    return resolveWithCustomWebOverlay(context, fdroidModuleOverrides[moduleName], platform);
  }

  return resolveWithCustomWebOverlay(context, moduleName, platform);
};

if (process.env.PASEO_SERVE_SIM_PREVIEW === "1") {
  const { simMiddleware } = require("serve-sim/middleware");
  const originalEnhanceMiddleware = config.server?.enhanceMiddleware;
  config.server = config.server ?? {};
  config.server.enhanceMiddleware = (metroMiddleware, server) => {
    const middleware = originalEnhanceMiddleware
      ? originalEnhanceMiddleware(metroMiddleware, server)
      : metroMiddleware;
    const serveSimulator = simMiddleware({
      basePath: "/.sim",
      device: process.env.PASEO_SERVE_SIM_DEVICE_UDID,
    });
    return (req, res, next) => {
      serveSimulator(req, res, (error) => {
        if (error) {
          if (next) {
            next(error);
            return;
          }
          throw error;
        }
        middleware(req, res, next);
      });
    };
  };
}

module.exports = config;
