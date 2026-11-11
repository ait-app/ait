import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

describe("desktop packaging", () => {
  it("uses an Electron runtime whose Squirrel handoff explicitly wakes ShipIt", () => {
    const pkg = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as {
      devDependencies?: Record<string, string>;
    };
    const electronVersion = pkg.devDependencies?.electron ?? "0.0.0";
    const electronMajor = Number(electronVersion.split(".")[0]);

    expect(electronMajor).toBeGreaterThanOrEqual(44);
  });

  it("requires macOS 13 or newer in the packaged application", () => {
    const config = readFileSync(join(packageRoot, "electron-builder.yml"), "utf8");

    expect(config).toContain('minimumSystemVersion: "13.0.0"');
  });

  it("excludes package debug/source files from the packaged app", () => {
    const config = readFileSync(join(packageRoot, "electron-builder.yml"), "utf8");

    expect(config).toContain("!**/*.map");
    expect(config).toContain("!node_modules/@ait/*/src/**");
    expect(config).toContain("!node_modules/@ait/**/*.test.*");
    expect(config).toContain("!node_modules/@ait/**/*.spec.*");
  });

  it("registers Ait agent links with the operating system", () => {
    const config = readFileSync(join(packageRoot, "electron-builder.yml"), "utf8");

    expect(config).toContain("name: Ait agent link");
    expect(config).toContain("- ait");
    expect(config).not.toContain("- paseo");
  });

  // Runtime dependencies must remain available inside app.asar.
  it("declares all workspace packages required at runtime", () => {
    const pkg = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as {
      dependencies?: Record<string, string>;
    };
    const deps = pkg.dependencies ?? {};

    expect(deps["@ait/protocol"]).toBe("file:../../packages/protocol");
    expect(deps["@ait/server"]).toBeUndefined();
    expect(deps["@ait/cli"]).toBeUndefined();
  });
});
