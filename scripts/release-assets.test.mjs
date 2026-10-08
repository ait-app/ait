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
import { AppInfo } from "app-builder-lib/out/appInfo.js";
import { Platform } from "app-builder-lib/out/core.js";
import { getPublishConfigs } from "app-builder-lib/out/publish/PublishManager.js";
import { createUpdateInfoTasks } from "app-builder-lib/out/publish/updateInfoBuilder.js";
import { expandMacro } from "app-builder-lib/out/util/macroExpander.js";
import { stringify, parse } from "yaml";
import { collectReleaseAssets, releaseAssetNames, verifyReleaseAssets } from "./release-assets.mjs";
import { releaseChannel } from "./release-version.mjs";
import { nightlyBuildLabel, nightlyBuilderArgs } from "./nightly-build.mjs";

async function builderPackager(platform, version, buildLabel) {
  const config = parse(
    await readFile(new URL("../apps/desktop/electron-builder.yml", import.meta.url), "utf8"),
  );
  if (buildLabel) {
    for (const arg of nightlyBuilderArgs(buildLabel)) {
      const [, key, value] = arg.match(/^-c\.([^=]+)=(.*)$/);
      const [section, property] = key.split(".");
      config[section][property] = value;
    }
  }
  const info = { config, metadata: { name: "ait", version } };
  const appInfo = new AppInfo(info);
  info.appInfo = appInfo;
  return {
    config,
    info,
    appInfo,
    platformSpecificBuildOptions: config[platform],
    platform: platform === "linux" ? Platform.LINUX : Platform.MAC,
    expandMacro: (value, arch) => expandMacro(value, arch, appInfo),
    getResource: async () => null,
  };
}

async function builderAssetNames(platform, version, buildLabel) {
  const packager = await builderPackager(platform, version, buildLabel);
  const { config } = packager;
  const arch = platform === "linux" ? Arch.x64 : Arch.arm64;
  const installers = config[platform].target.map((ext) => {
    const options = ext === "AppImage" ? config.appImage : config[platform];
    return expandMacro(options.artifactName, getArtifactArchName(arch, ext), { version }, { ext });
  });
  const [publish] = await getPublishConfigs(packager, null, arch, true);
  return [...installers, `${publish.channel}-${platform}.yml`];
}

async function fixture(t, platform, version = "0.0.7", buildLabel) {
  const root = await mkdtemp(path.join(tmpdir(), "ait-release-assets-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = path.join(root, "source");
  const destination = path.join(root, "release");
  await mkdir(source);
  // Generate fixtures from builder's naming rules, independently of the collector allowlist.
  const names = await builderAssetNames(platform, version, buildLabel);
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
  return { platform, version, source, destination, buildLabel };
}

async function addAndroidAssets(directory) {
  await writeFile(path.join(directory, "Ait-0.0.7-android.apk"), "universal APK");
}

test("release assets match electron-builder's target-specific architecture names", async () => {
  for (const version of ["0.0.7", "0.0.8-beta.1"]) {
    for (const platform of ["linux", "mac"]) {
      assert.deepEqual(
        releaseAssetNames(platform, version),
        await builderAssetNames(platform, version),
      );
    }
  }
});

test("nightly identity uses eight hash characters and a deterministic UTC commit date", () => {
  const sha = "abcdef01".repeat(5);
  const timestamp = Date.parse("2026-10-08T00:15:00+02:00") / 1000;
  assert.equal(nightlyBuildLabel(sha, timestamp), "abcdef01-2026-10-07");
  assert.throws(() => nightlyBuildLabel("bad", timestamp));
  assert.throws(() => nightlyBuildLabel(sha, NaN));
  assert.throws(() => nightlyBuilderArgs("../../invalid"));
});

test("nightly builder names and update URLs use the build label on both platforms", async (t) => {
  const version = "0.0.23-beta.1";
  const buildLabel = "abcdef01-2026-10-08";
  const inputs = [];
  for (const platform of ["linux", "mac"]) {
    const input = await fixture(t, platform, version, buildLabel);
    inputs.push(input);
    const packager = await builderPackager(platform, version, buildLabel);
    const arch = platform === "linux" ? Arch.x64 : Arch.arm64;
    const names = releaseAssetNames(platform, version, buildLabel);
    assert.deepEqual(names, await builderAssetNames(platform, version, buildLabel));
    assert.ok(
      names.slice(0, 2).every((name) => name.includes(buildLabel) && !name.includes(version)),
    );
    const tasks = await createUpdateInfoTasks(
      {
        packager,
        arch,
        file: path.join(input.source, names[platform === "mac" ? 1 : 0]),
        target: { outDir: input.source },
      },
      await getPublishConfigs(packager, null, arch, true),
    );
    assert.equal(tasks.length, 1);
    assert.equal(path.basename(tasks[0].file), `${releaseChannel(version)}-${platform}.yml`);
    assert.equal(tasks[0].info.version, version); // Required by the updater's SemVer parser.
    assert.ok(tasks[0].info.files.every((file) => file.url.includes(buildLabel)));
    await writeFile(path.join(input.source, names[0] + ".blockmap"), "block map");
    await collectReleaseAssets(input);
    assert.ok((await readdir(input.destination)).includes(names[0] + ".blockmap"));
    await assert.rejects(collectReleaseAssets({ ...input, buildLabel: undefined }));
  }
  const directory = inputs[0].destination;
  for (const name of await readdir(inputs[1].destination))
    await copyFile(path.join(inputs[1].destination, name), path.join(directory, name));
  const info = {
    version: buildLabel,
    packagedVersion: version,
    releaseTag: "nightly",
    sourceCommit: "abcdef01".repeat(5),
    workflowCommit: "abcdef01".repeat(5),
  };
  await writeFile(path.join(directory, "BUILD-INFO.json"), JSON.stringify(info));
  await verifyReleaseAssets({ version, buildLabel, directory });
  const cli = spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL("./release-assets.mjs", import.meta.url)),
      "verify",
      version,
      directory,
      "--nightly",
      buildLabel,
    ],
    { encoding: "utf8" },
  );
  assert.equal(cli.status, 0, cli.stderr);
  const checksums = await readFile(path.join(directory, "SHA256SUMS"), "utf8");
  assert.match(checksums, /Ait-abcdef01-2026-10-08-linux-x86_64.AppImage/);
  assert.match(checksums, /Ait-abcdef01-2026-10-08-macos-arm64.dmg.blockmap/);
  assert.doesNotMatch(checksums, /0\.0\.23/);
  await writeFile(
    path.join(directory, "BUILD-INFO.json"),
    JSON.stringify({ ...info, version: "obsolete" }),
  );
  await assert.rejects(
    verifyReleaseAssets({ version, buildLabel, directory }),
    /Build information version/,
  );
});

test("accepts stable and numbered beta versions but rejects ambiguous release channels", () => {
  assert.equal(releaseChannel("0.0.7"), "latest");
  assert.equal(releaseChannel("0.0.8-beta.12"), "beta");
  for (const version of [
    "v0.0.7",
    "00.0.7",
    "0.0.8-beta",
    "0.0.8-beta.0",
    "0.0.8-beta.01",
    "0.0.8-alpha.1",
    "0.0.8+build.1",
    "0.0.8\n",
  ]) {
    assert.throws(() => releaseChannel(version), /Release version must use/);
  }
});

test("builder generates beta updater files and collector rejects stable metadata in a beta release", async (t) => {
  const version = "0.0.8-beta.1";
  const linux = await fixture(t, "linux", version);
  const mac = await fixture(t, "mac", version);
  for (const input of [linux, mac]) {
    const packager = await builderPackager(input.platform, version);
    const arch = input.platform === "linux" ? Arch.x64 : Arch.arm64;
    const publish = await getPublishConfigs(packager, null, arch, true);
    const names = releaseAssetNames(input.platform, version);
    const tasks = await createUpdateInfoTasks(
      {
        packager,
        arch,
        file: path.join(input.source, names[input.platform === "mac" ? 1 : 0]),
        target: { outDir: input.source },
      },
      publish,
    );
    assert.deepEqual(
      tasks.map((task) => path.basename(task.file)),
      [names[2]],
    );
    assert.equal(tasks[0].info.version, version);
    await collectReleaseAssets({ ...input, destination: linux.destination });
  }
  await verifyReleaseAssets({ version, directory: linux.destination });
  await writeFile(path.join(linux.destination, "latest-linux.yml"), "version: 0.0.7");
  await assert.rejects(
    verifyReleaseAssets({ version, directory: linux.destination }),
    /Unexpected release asset: latest-linux.yml/,
  );
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

test("desktop release verifies published desktop assets and any retained Android APK", async (t) => {
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
  await copyFile(
    new URL("./release-version.mjs", import.meta.url),
    path.join(tooling, "release-version.mjs"),
  );
  const workflow = parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  const step = workflow.jobs.release.steps.find(
    (item) => item.name === "Verify assets and write checksums",
  );
  const run = () =>
    spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
      cwd: root,
      env: { ...process.env, RELEASE_TAG: "v0.0.7" },
      encoding: "utf8",
    });
  assert.equal(run().status, 0);
  await addAndroidAssets(assets);
  assert.equal(run().status, 0);
  assert.match(await readFile(path.join(assets, "SHA256SUMS"), "utf8"), /Ait-0\.0\.7-android\.apk/);
});

test("desktop reruns preserve a checksum-verified Android APK", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-retain-android-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const directory of ["bin", "published", "release-assets"])
    await mkdir(path.join(root, directory));
  const apk = "Ait-0.0.7-android.apk";
  const original = "previously published APK";
  await writeFile(path.join(root, "published", apk), original);
  const hash = createHash("sha256").update(original).digest("hex");
  await writeFile(path.join(root, "published/SHA256SUMS"), `${hash}  ${apk}\n`);
  await writeFile(
    path.join(root, "bin/gh"),
    `#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
if (args[0] === 'release' && args[1] === 'download') {
  const destination = args[args.indexOf('--dir') + 1];
  for (const name of fs.readdirSync(process.env.PUBLISHED_ASSETS)) {
    fs.copyFileSync(path.join(process.env.PUBLISHED_ASSETS, name), path.join(destination, name));
  }
}
`,
    { mode: 0o700 },
  );
  const workflow = parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  const step = workflow.jobs.release.steps.find(
    (item) => item.name === "Preserve previously published Android APK",
  );
  const run = () =>
    spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
      cwd: root,
      env: {
        ...process.env,
        PATH: `${root}/bin:${process.env.PATH}`,
        PUBLISHED_ASSETS: path.join(root, "published"),
        RELEASE_TAG: "v0.0.7",
        GITHUB_REPOSITORY: "example/ait",
      },
      encoding: "utf8",
    });
  assert.equal(run().status, 0);
  assert.equal(await readFile(path.join(root, "release-assets", apk), "utf8"), original);
  await writeFile(path.join(root, "published", apk), "tampered");
  assert.notEqual(run().status, 0);
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

test("GitHub creation and repair keep beta prereleases out of latest stable", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-beta-publish-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, "bin"));
  await mkdir(path.join(root, "release-assets"));
  await writeFile(path.join(root, "release-assets/SHA256SUMS"), "fixture");
  const calls = path.join(root, "gh-calls.jsonl");
  await writeFile(
    path.join(root, "bin/gh"),
    `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2);
fs.appendFileSync(process.env.GH_CALLS, JSON.stringify(args) + '\\n');
if (args[1] === 'view' && process.env.RELEASE_EXISTS !== '1') process.exit(1);
`,
    { mode: 0o700 },
  );
  const workflow = parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  const step = workflow.jobs.release.steps.find(
    (item) => item.name === "Create or repair GitHub Release",
  );
  for (const tag of ["v0.0.7", "v0.0.8-beta.1"]) {
    for (const exists of [false, true]) {
      await writeFile(calls, "");
      const result = spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
        cwd: root,
        env: {
          ...process.env,
          PATH: `${root}/bin:${process.env.PATH}`,
          GH_CALLS: calls,
          RELEASE_EXISTS: exists ? "1" : "0",
          RELEASE_TAG: tag,
          GITHUB_REPOSITORY: "example/ait",
        },
        encoding: "utf8",
      });
      assert.equal(result.status, 0, result.stderr);
      const commands = (await readFile(calls, "utf8"))
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      const publish = commands.find((args) => args[1] === (exists ? "edit" : "create"));
      if (tag.includes("-beta.")) {
        assert(publish, "beta flags must be set on both creation and repair");
        assert(publish.includes("--prerelease"));
        assert(publish.includes("--latest=false"));
      } else {
        assert(
          !commands.some(
            (args) => args.includes("--prerelease") || args.includes("--latest=false"),
          ),
        );
      }
      if (exists) assert(commands.some((args) => args[1] === "upload"));
    }
  }
});
