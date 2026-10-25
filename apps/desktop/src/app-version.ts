import { app } from "electron";
import { readFileSync } from "node:fs";
import path from "node:path";

export function displayAppVersion(): string {
  try {
    const pkg = JSON.parse(readFileSync(path.join(app.getAppPath(), "package.json"), "utf8")) as {
      displayVersion?: string;
    };
    return pkg.displayVersion || app.getVersion();
  } catch {
    return app.getVersion();
  }
}
