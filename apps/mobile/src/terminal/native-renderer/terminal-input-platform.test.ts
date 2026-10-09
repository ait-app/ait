import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const { resolve } = require("metro-resolver");
const { resolver } = require("../../../metro.config.cjs");

describe("native terminal input platform resolution", () => {
  it.each([
    ["ios", "terminal-input.ios.tsx"],
    ["android", "terminal-input.native.tsx"],
  ])("selects the intended input on %s with the app's Metro configuration", (platform, file) => {
    const result = resolve(
      {
        originModulePath: path.join(import.meta.dirname, "terminal-input-platform.test.ts"),
        sourceExts: resolver.sourceExts,
        preferNativePlatform: true,
        isAssetFile: () => false,
        redirectModulePath: (modulePath: string) => modulePath,
        getPackageForModule: () => null,
        fileSystemLookup: (filePath: string) => {
          if (!fs.existsSync(filePath)) return { exists: false };
          return {
            exists: true,
            type: fs.statSync(filePath).isDirectory() ? "d" : "f",
            realPath: fs.realpathSync(filePath),
          };
        },
      },
      "./terminal-input",
      platform,
    );
    expect(result).toEqual({
      type: "sourceFile",
      filePath: path.join(import.meta.dirname, file),
    });
  });
});
