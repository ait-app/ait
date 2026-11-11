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
  // Only immutable sources predating the bridge may omit it. A missing current
  // manifest is corruption, not permission to publish a host-only archive.
  const workspace = readFileSync(path.join(root, "Cargo.toml"), "utf8");
  const bridgeManifest = path.join(root, "bins/omp-acp/Cargo.toml");
  layout.ompBridge = existsSync(bridgeManifest);
  assert(
    layout.ompBridge || !workspace.includes('"bins/omp-acp"'),
    "Current source is missing the OMP ACP bridge manifest",
  );
  if (layout.ompBridge) {
    assert.match(readFileSync(bridgeManifest, "utf8"), /^name\s*=\s*"ait-omp-acp"$/m);
  }
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
