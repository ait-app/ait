import assert from "node:assert/strict";
import { existsSync, readFileSync, appendFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

// Release workflows may rebuild immutable tags from before the workspace rename.
export function resolveReleaseLayout(root) {
  const renamed = existsSync(path.join(root, "bins/daemon/Cargo.toml"));
  const layout = renamed
    ? { package: "daemon", binary: "daemon", desktop: "apps/desktop" }
    : { package: "server-bin", binary: "server", desktop: "apps/paseo" };
  const manifest = readFileSync(path.join(root, "bins", layout.binary, "Cargo.toml"), "utf8");
  assert.match(manifest, new RegExp(`^name\\s*=\\s*"${layout.package}"$`, "m"));
  assert.match(manifest, new RegExp(`^name\\s*=\\s*"${layout.binary}"$`, "m"));
  assert(existsSync(path.join(root, layout.desktop, "package.json")), "Missing desktop workspace");
  return layout;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const layout = resolveReleaseLayout(process.cwd());
  assert(process.env.GITHUB_OUTPUT, "GITHUB_OUTPUT must be set by the release workflow");
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    Object.entries(layout)
      .map(([key, value]) => `${key}=${value}\n`)
      .join(""),
  );
}
