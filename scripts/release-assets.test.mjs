import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFile,
  mkdtemp,
  mkdir,
  readdir,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { Arch, getArtifactArchName } from "builder-util";
import { expandMacro } from "app-builder-lib/out/util/macroExpander.js";
import { stringify, parse } from "yaml";
import { collectReleaseAssets, releaseAssetNames, verifyReleaseAssets } from "./release-assets.mjs";

async function builderAssetNames(platform, version) {
  const config = parse(
    await readFile(new URL("../apps/desktop/electron-builder.yml", import.meta.url), "utf8"),
  );
  const arch = platform === "linux" ? Arch.x64 : Arch.arm64;
  const installers = config[platform].target.map((ext) => {
    const options = ext === "AppImage" ? config.appImage : config[platform];
    return expandMacro(options.artifactName, getArtifactArchName(arch, ext), { version }, { ext });
  });
  return [...installers, `latest-${platform}.yml`];
}

async function fixture(t, platform) {
  const root = await mkdtemp(path.join(tmpdir(), "ait-release-assets-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = path.join(root, "source");
  const destination = path.join(root, "release");
  await mkdir(source);
  const version = "0.0.7";
  // Generate fixtures from builder's naming rules, independently of the collector allowlist.
  const names = await builderAssetNames(platform, version);
  const files = [];
  for (const name of names.slice(0, 2)) {
    const contents = Buffer.from(`packaged installer: ${name}`);
    await writeFile(path.join(source, name), contents);
    files.push({
      url: name,
      sha512: createHash("sha512").update(contents).digest("base64"),
      size: contents.length,
    });
  }
  await writeFile(path.join(source, names[2]), stringify({ version, files }));
  await writeFile(path.join(source, "old-server.exe"), "should never be collected");
  return { platform, version, source, destination };
}

async function addAndroidAssets(directory) {
  await writeFile(path.join(directory, "Ait-0.0.7-android.apk"), "universal APK");
}

test("release assets match electron-builder's target-specific architecture names", async () => {
  for (const platform of ["linux", "mac"]) {
    assert.deepEqual(
      releaseAssetNames(platform, "0.0.7"),
      await builderAssetNames(platform, "0.0.7"),
    );
  }
});

test("checksums desktop and Android installers, updater metadata and blockmaps", async (t) => {
  const linux = await fixture(t, "linux");
  const mac = await fixture(t, "mac");
  const blockmap = "Ait-0.0.7-macos-arm64.zip.blockmap";
  await writeFile(path.join(mac.source, blockmap), "blockmap");
  await collectReleaseAssets(linux);
  await collectReleaseAssets({ ...mac, destination: linux.destination });
  await addAndroidAssets(linux.destination);
  const names = await verifyReleaseAssets({
    version: linux.version,
    directory: linux.destination,
    includeAndroid: true,
  });
  assert.equal(names.length, 8);
  assert(names.includes(blockmap));
  const lines = (await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8"))
    .trim()
    .split("\n");
  assert.equal(lines.length, names.length);
  for (const name of names) {
    const hash = createHash("sha256")
      .update(await readFile(path.join(linux.destination, name)))
      .digest("hex");
    assert(lines.includes(`${hash}  ${name}`));
  }
  assert(!(await readdir(linux.destination)).includes("old-server.exe"));
});

test("rejects stale updater checksums and missing installers", async (t) => {
  const input = await fixture(t, "linux");
  await writeFile(path.join(input.source, "Ait-linux-x86_64.AppImage"), "changed");
  await assert.rejects(collectReleaseAssets(input), /checksum mismatch/);
  await rm(path.join(input.source, "Ait-0.0.7-linux-x64.tar.gz"));
  await assert.rejects(collectReleaseAssets(input), /ENOENT/);
});

test("desktop-only verification is the default and Android requires an explicit option", async (t) => {
  const linux = await fixture(t, "linux");
  const mac = await fixture(t, "mac");
  await collectReleaseAssets(linux);
  await collectReleaseAssets({ ...mac, destination: linux.destination });
  const script = fileURLToPath(new URL("./release-assets.mjs", import.meta.url));
  const verify = (...options) =>
    spawnSync(process.execPath, [script, "verify", linux.version, linux.destination, ...options], {
      encoding: "utf8",
    });
  assert.equal(verify().status, 0);
  assert.equal(
    (await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8")).trim().split("\n").length,
    6,
  );
  assert.notEqual(verify("--android").status, 0);
  await addAndroidAssets(linux.destination);
  assert.notEqual(verify().status, 0, "unrequested Android artifacts must not be published");
  assert.equal(verify("--android").status, 0);
  assert.equal(
    (await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8")).trim().split("\n").length,
    7,
  );
  assert.notEqual(verify("--andriod").status, 0, "unknown options must fail closed");
});

test("rejects incomplete releases and unintended platform assets", async (t) => {
  const input = await fixture(t, "linux");
  await collectReleaseAssets(input);
  await assert.rejects(
    verifyReleaseAssets({ version: input.version, directory: input.destination }),
    /Missing release asset/,
  );
  const mac = await fixture(t, "mac");
  await collectReleaseAssets({ ...mac, destination: input.destination });
  const options = { version: input.version, directory: input.destination, includeAndroid: true };
  await assert.rejects(
    verifyReleaseAssets(options),
    /Missing release asset: Ait-0.0.7-android.apk/,
  );
  await addAndroidAssets(input.destination);
  await rm(path.join(input.destination, "Ait-0.0.7-android.apk"));
  await assert.rejects(
    verifyReleaseAssets(options),
    /Missing release asset: Ait-0.0.7-android.apk/,
  );
  await addAndroidAssets(input.destination);
  await writeFile(path.join(input.destination, "Ait-Setup.exe"), "unexpected");
  await assert.rejects(verifyReleaseAssets(options), /Unexpected release asset/);
});

test("release workflow passes the selected platforms to the asset verifier", async (t) => {
  const linux = await fixture(t, "linux");
  const mac = await fixture(t, "mac");
  await collectReleaseAssets(linux);
  await collectReleaseAssets({ ...mac, destination: linux.destination });
  const root = path.dirname(linux.destination);
  const assets = path.join(root, "release-assets");
  await rename(linux.destination, assets);
  const tooling = path.join(root, ".tmp/release-tools/scripts");
  await mkdir(tooling, { recursive: true });
  await copyFile(
    new URL("./release-assets.mjs", import.meta.url),
    path.join(tooling, "release-assets.mjs"),
  );
  const workflow = parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  const step = workflow.jobs.release.steps.find(
    (item) => item.name === "Verify assets and write checksums",
  );
  const run = (selected) =>
    spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
      cwd: root,
      env: { ...process.env, RELEASE_TAG: "v0.0.7", BUILD_ANDROID: selected },
      encoding: "utf8",
    });
  assert.equal(run("false").status, 0);
  assert.notEqual(run("true").status, 0);
  await addAndroidAssets(assets);
  assert.equal(run("true").status, 0);
  assert.notEqual(run("false").status, 0);
});

test("release workflow builds only the daemon and packages the resolved desktop workspace", async () => {
  const workflow = parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  assert.deepEqual(
    workflow.jobs.build.strategy.matrix.include.map((entry) => entry.platform).sort(),
    ["linux", "mac"],
  );
  const build = workflow.jobs.build.steps.find((step) => step.name === "Build release daemon only");
  assert.match(build.run, /--locked --release -p "\$\{\{ steps.layout.outputs.package \}\}"/);
  assert.match(build.run, /--bin "\$\{\{ steps.layout.outputs.binary \}\}" --target/);
  const collect = workflow.jobs.build.steps.find(
    (step) => step.name === "Collect installers and updater assets",
  );
  assert.match(collect.run, /steps.layout.outputs.desktop/);
  const config = parse(
    await readFile(new URL("../apps/desktop/electron-builder.yml", import.meta.url), "utf8"),
  );
  assert.equal(config.appId, "dev.ait.desktop");
  assert.deepEqual(
    config.extraResources.filter((entry) => entry.to.startsWith("bin")),
    [{ from: "release-resources/daemon/daemon", to: "bin/daemon" }],
  );
  assert.deepEqual(config.mac.binaries, ["Contents/Resources/bin/daemon"]);
  assert.equal(config.win, undefined);
  assert.deepEqual(config.linux.target, ["AppImage", "tar.gz"]);
});

test("checksums build provenance and rejects a mismatched source version", async (t) => {
  const linux = await fixture(t, "linux");
  const mac = await fixture(t, "mac");
  await collectReleaseAssets(linux);
  await collectReleaseAssets({ ...mac, destination: linux.destination });
  const info = {
    version: "0.0.7",
    releaseTag: "v0.0.7",
    sourceCommit: "a".repeat(40),
    workflowCommit: "b".repeat(40),
  };
  const file = path.join(linux.destination, "BUILD-INFO.json");
  await writeFile(file, JSON.stringify(info));
  await verifyReleaseAssets({ version: linux.version, directory: linux.destination });
  assert.match(
    await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8"),
    /  BUILD-INFO\.json/,
  );
  await writeFile(file, JSON.stringify({ ...info, version: "0.0.6" }));
  await assert.rejects(
    verifyReleaseAssets({ version: linux.version, directory: linux.destination }),
    /version differs/,
  );
  await writeFile(file, JSON.stringify({ ...info, sourceCommit: "main" }));
  await assert.rejects(
    verifyReleaseAssets({ version: linux.version, directory: linux.destination }),
    /full commit SHA/,
  );
});
