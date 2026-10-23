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
import {
  getAppUpdatePublishConfiguration,
  getPublishConfigs,
} from "app-builder-lib/out/publish/PublishManager.js";
import { createUpdateInfoTasks } from "app-builder-lib/out/publish/updateInfoBuilder.js";
import { expandMacro } from "app-builder-lib/out/util/macroExpander.js";
import { HttpError } from "builder-util-runtime";
import { createClient } from "electron-updater/out/providerFactory.js";
import { stringify, parse } from "yaml";
import {
  collectReleaseAssets,
  daemonAssetName,
  daemonAssetNames,
  packDaemonAsset,
  releaseAssetNames,
  verifyReleaseAssets,
} from "./release-assets.mjs";
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
    platform: { linux: Platform.LINUX, mac: Platform.MAC, win: Platform.WINDOWS }[platform],
    expandMacro: (value, arch) => expandMacro(value, arch, appInfo),
    getResource: async () => null,
  };
}

async function builderAssetNames(platform, version, buildLabel) {
  const packager = await builderPackager(platform, version, buildLabel);
  const { config } = packager;
  const arch = platform === "mac" ? Arch.arm64 : Arch.x64;
  const installers = config[platform].target.map((target) => {
    const ext = target === "nsis" ? "exe" : target;
    const options =
      target === "nsis" ? config.nsis : ext === "AppImage" ? config.appImage : config[platform];
    return expandMacro(options.artifactName, getArtifactArchName(arch, ext), { version }, { ext });
  });
  const [publish] = await getPublishConfigs(packager, null, arch, true);
  return [...installers, `${publish.channel}${platform === "win" ? "" : `-${platform}`}.yml`];
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
  for (const name of names.slice(0, -1)) {
    const contents = Buffer.from(`packaged installer: ${name}`);
    await writeFile(path.join(source, name), contents);
    files.push({
      url: name,
      sha512: createHash("sha512").update(contents).digest("base64"),
      size: contents.length,
    });
  }
  await writeFile(path.join(source, names.at(-1)), stringify({ version, files }));
  await writeFile(path.join(source, "old-server.exe"), "should never be collected");
  return { platform, version, source, destination, buildLabel };
}

async function addAndroidAssets(directory) {
  await writeFile(path.join(directory, "Ait-0.0.7-android.apk"), "universal APK");
}

async function addDaemonAssets(directory, version = "0.0.7", buildLabel) {
  for (const name of daemonAssetNames(version, buildLabel))
    await writeFile(path.join(directory, name), "daemon archive");
}

async function fakeDaemon(t, reported) {
  const root = await mkdtemp(path.join(tmpdir(), "ait-daemon-binary-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const binary = path.join(root, "daemon");
  await writeFile(binary, `#!/bin/sh\necho "${reported}"\n`, { mode: 0o700 });
  return { root, binary };
}

test("release assets match electron-builder's target-specific architecture names", async () => {
  for (const version of ["0.0.7", "0.0.8-beta.1"]) {
    for (const platform of ["linux", "mac", "win"]) {
      assert.deepEqual(
        releaseAssetNames(platform, version),
        await builderAssetNames(platform, version),
      );
    }
  }
});

test("macOS embeds the AIT Server feed for stable and beta releases", async () => {
  for (const version of ["0.0.7", "0.0.8-beta.1"]) {
    const mac = await getAppUpdatePublishConfiguration(
      await builderPackager("mac", version),
      null,
      Arch.arm64,
      true,
    );
    assert.equal(mac.provider, "generic");
    assert.equal(mac.url, "https://aitapp.cn/api/v1/updates/macos/");
    assert.equal(mac.channel, releaseChannel(version));
    assert.equal(mac.useMultipleRangeRequest, false);

    const linux = await getAppUpdatePublishConfiguration(
      await builderPackager("linux", version),
      null,
      Arch.x64,
      true,
    );
    assert.equal(linux.provider, "github");
    assert.equal(linux.owner, "ait-app");
    assert.equal(linux.repo, "ait");
    assert.equal(linux.channel, releaseChannel(version));
  }
});

test("macOS updater reads server JSON and resolves versioned files and blockmaps", async () => {
  const config = await getAppUpdatePublishConfiguration(
    await builderPackager("mac", "0.0.7"),
    null,
    Arch.arm64,
    true,
  );
  // Match the runtime's stable/latest and beta mapping, including changing channels.
  const updater = { channel: "latest", isAddNoCacheQuery: true };
  let manifest;
  let requested;
  const provider = createClient(config, updater, {
    platform: "darwin",
    executor: {
      request: async (options) => {
        requested = new URL(`${options.protocol}//${options.hostname}${options.path}`);
        return JSON.stringify(manifest);
      },
    },
  });
  assert.equal(provider.isUseMultipleRangeRequest, false);
  for (const version of ["0.0.8", "0.0.9-beta.1", "0.0.8"]) {
    updater.channel = releaseChannel(version);
    const name = `Ait-${version}-macos-arm64.zip`;
    manifest = {
      version,
      files: [{ url: `files/v${version}/${name}`, sha512: "a".repeat(86) + "==", size: 1234 }],
      path: `files/v${version}/${name}`,
      releaseDate: "2026-10-18T00:00:00.000Z",
      releaseNotes: "Release notes from AIT Server",
      rolloutHours: 24,
    };

    const info = await provider.getLatestVersion();

    assert.equal(requested.origin, "https://aitapp.cn");
    assert.equal(requested.pathname, `/api/v1/updates/macos/${updater.channel}-mac.yml`);
    assert(requested.searchParams.has("noCache"));
    assert.deepEqual(info, manifest);
    const [file] = provider.resolveFiles(info);
    assert.equal(file.url.href, `${config.url}files/v${version}/${name}`);
    assert.deepEqual(file.info, manifest.files[0]);
    const [oldBlockmap, newBlockmap] = provider.getBlockMapFiles(file.url, "0.0.7", version);
    assert.equal(newBlockmap.href, `${file.url.href}.blockmap`);
    assert.equal(oldBlockmap.href, `${config.url}files/v0.0.7/Ait-0.0.7-macos-arm64.zip.blockmap`);
  }
});

test("macOS server feed distinguishes unpublished channels from service failures", async () => {
  const config = await getAppUpdatePublishConfiguration(
    await builderPackager("mac", "0.0.8-beta.1"),
    null,
    Arch.arm64,
    true,
  );
  for (const status of [404, 503]) {
    const error = new HttpError(status, "server response");
    const provider = createClient(
      config,
      { channel: "beta" },
      {
        platform: "darwin",
        executor: {
          request: async () => {
            throw error;
          },
        },
      },
    );

    await assert.rejects(provider.getLatestVersion(), (result) => {
      if (status === 404) assert.equal(result.code, "ERR_UPDATER_CHANNEL_FILE_NOT_FOUND");
      else assert.equal(result, error);
      return true;
    });
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
      names.slice(0, -1).every((name) => name.includes(buildLabel) && !name.includes(version)),
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
  await assert.rejects(
    verifyReleaseAssets({ version, buildLabel, directory }),
    new RegExp(`Missing release asset: Ait-${buildLabel}-daemon-linux-x64\\.tar\\.gz`),
  );
  await addDaemonAssets(directory, version, buildLabel);
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
      [names.at(-1)],
    );
    assert.equal(tasks[0].info.version, version);
    await collectReleaseAssets({
      ...input,
      destination: linux.destination,
    });
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
  assert.equal(names.length, 10);
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

test("Windows collection validates the EXE update hash and retains its blockmap", async (t) => {
  for (const version of ["0.1.15", "0.1.16-beta.1"]) {
    const input = await fixture(t, "win", version);
    const [installer, , metadata] = releaseAssetNames("win", version);
    const packager = await builderPackager("win", version);
    const tasks = await createUpdateInfoTasks(
      {
        packager,
        arch: Arch.x64,
        file: path.join(input.source, installer),
        target: { outDir: input.source },
      },
      await getPublishConfigs(packager, null, Arch.x64, true),
    );
    assert.equal(path.basename(tasks[0].file), metadata);
    await writeFile(path.join(input.source, `${installer}.blockmap`), "blockmap");
    const collected = await collectReleaseAssets(input);
    assert(collected.includes(`${installer}.blockmap`));
    assert(!collected.includes("old-server.exe"));
    await writeFile(path.join(input.source, installer), "tampered EXE");
    await assert.rejects(collectReleaseAssets(input), /checksum mismatch/);
  }
});

test("release CI requires Windows assets while historical releases and repairs stay valid", async (t) => {
  const linux = await fixture(t, "linux");
  const mac = await fixture(t, "mac");
  await collectReleaseAssets(linux);
  await collectReleaseAssets({ ...mac, destination: linux.destination });
  const options = { version: linux.version, directory: linux.destination };
  await verifyReleaseAssets(options);
  await assert.rejects(
    verifyReleaseAssets({ ...options, includeWindows: true }),
    /Missing release asset: .*windows/,
  );
  const windows = await fixture(t, "win");
  await collectReleaseAssets({ ...windows, destination: linux.destination });
  await writeFile(
    path.join(linux.destination, "BUILD-INFO-windows.json"),
    JSON.stringify({
      version: linux.version,
      releaseTag: `v${linux.version}`,
      sourceCommit: "a".repeat(40),
      workflowCommit: "b".repeat(40),
    }),
  );
  await verifyReleaseAssets({ ...options, includeWindows: true });
  // A repair detects and preserves a complete Windows asset set without new flags.
  await verifyReleaseAssets(options);
  const sums = await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8");
  assert.match(sums, /Ait-0\.0\.7-windows-x64-setup\.exe/);
  assert.match(sums, /BUILD-INFO-windows\.json/);
  await rm(path.join(linux.destination, "Ait-0.0.7-windows-x64.zip"));
  await assert.rejects(verifyReleaseAssets(options), /Missing release asset: .*windows.*zip/);
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
    8,
  );
  assert.notEqual(verify("--android").status, 0);
  await addAndroidAssets(linux.destination);
  assert.notEqual(verify().status, 0, "unrequested Android artifacts must not be published");
  assert.equal(verify("--android").status, 0);
  assert.equal(
    (await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8")).trim().split("\n").length,
    9,
  );
  assert.notEqual(verify("--andriod").status, 0, "unknown options must fail closed");
});

test("standalone daemon archives package ait-daemon and the license per target", async (t) => {
  assert.equal(daemonAssetName("linux", "0.0.7"), "Ait-0.0.7-daemon-linux-x64.tar.gz");
  assert.equal(
    daemonAssetName("mac", "0.0.8-beta.1"),
    "Ait-0.0.8-beta.1-daemon-macos-arm64.tar.gz",
  );
  assert.throws(() => daemonAssetName("android", "0.0.7"), /Unsupported daemon platform/);
  const { root, binary } = await fakeDaemon(t, "daemon 0.0.7");
  const license = path.join(root, "LICENSE");
  await writeFile(license, "license text");
  const destination = path.join(root, "release-assets");
  const name = await packDaemonAsset({
    platform: "linux",
    version: "0.0.7",
    binary,
    destination,
    license,
  });
  assert.equal(name, "Ait-0.0.7-daemon-linux-x64.tar.gz");
  const listing = spawnSync("tar", ["-tvzf", path.join(destination, name)], {
    encoding: "utf8",
  });
  assert.equal(listing.status, 0, listing.stderr);
  const entries = listing.stdout.trim().split("\n");
  assert.deepEqual(
    entries.map((line) => line.split(/\s+/).at(-1)),
    ["ait-daemon", "LICENSE"],
  );
  assert.match(entries[0], /^-rwxr-xr-x/);
  const stale = await fakeDaemon(t, "daemon 0.0.6");
  await assert.rejects(
    packDaemonAsset({
      platform: "mac",
      version: "0.0.7",
      binary: stale.binary,
      destination,
    }),
    /Daemon binary reports daemon 0\.0\.6/,
  );
  assert.deepEqual((await readdir(destination)).sort(), [name]);
  const script = fileURLToPath(new URL("./release-assets.mjs", import.meta.url));
  const invalid = spawnSync(
    process.execPath,
    [script, "daemon", "linux", "0.0.7", binary, destination, "--nightly", "abc"],
    { encoding: "utf8" },
  );
  assert.notEqual(invalid.status, 0, "nightly daemons need a valid build label");
  // Nightly archives are named by build label but still check the packaged SemVer.
  const nightly = spawnSync(
    process.execPath,
    [script, "daemon", "mac", "0.0.7", binary, destination, "--nightly", "abcdef01-2026-10-08"],
    { encoding: "utf8" },
  );
  assert.equal(nightly.status, 0, nightly.stderr);
  assert.equal(nightly.stdout.trim(), "Ait-abcdef01-2026-10-08-daemon-macos-arm64.tar.gz");
  assert.equal(
    daemonAssetName("linux", "0.0.7", "abcdef01-2026-10-08"),
    "Ait-abcdef01-2026-10-08-daemon-linux-x64.tar.gz",
  );
});

test("stable verification requires daemon archives only when requested", async (t) => {
  const linux = await fixture(t, "linux");
  const mac = await fixture(t, "mac");
  await collectReleaseAssets(linux);
  await collectReleaseAssets({ ...mac, destination: linux.destination });
  const options = {
    version: linux.version,
    directory: linux.destination,
    includeDaemon: true,
  };
  await assert.rejects(
    verifyReleaseAssets(options),
    /Missing release asset: Ait-0\.0\.7-daemon-linux-x64\.tar\.gz/,
  );
  await addDaemonAssets(linux.destination);
  await verifyReleaseAssets(options);
  const sums = await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8");
  assert.equal(sums.trim().split("\n").length, 10);
  assert.match(sums, /  Ait-0\.0\.7-daemon-macos-arm64\.tar\.gz/);
  // Android-only repairs re-verify a published release without rebuilding its daemons.
  await addAndroidAssets(linux.destination);
  await verifyReleaseAssets({
    ...options,
    includeDaemon: false,
    includeAndroid: true,
  });
  // Version-named daemon archives never belong in a nightly.
  await assert.rejects(
    verifyReleaseAssets({ ...options, includeAndroid: false, buildLabel: "38fb6054-2026-10-08" }),
    /Missing release asset|Unexpected release asset/,
  );
});

test("rejects incomplete releases and unintended platform assets", async (t) => {
  const input = await fixture(t, "linux");
  await collectReleaseAssets(input);
  await assert.rejects(
    verifyReleaseAssets({
      version: input.version,
      directory: input.destination,
    }),
    /Missing release asset/,
  );
  const mac = await fixture(t, "mac");
  await collectReleaseAssets({ ...mac, destination: input.destination });
  const options = {
    version: input.version,
    directory: input.destination,
    includeAndroid: true,
  };
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
  assert.notEqual(run().status, 0, "stable releases must carry standalone daemon archives");
  await addDaemonAssets(assets);
  assert.notEqual(run().status, 0, "release CI must require Windows installers");
  const windows = await fixture(t, "win");
  await collectReleaseAssets({ ...windows, destination: assets });
  assert.equal(run().status, 0);
  assert.match(
    await readFile(path.join(assets, "SHA256SUMS"), "utf8"),
    /Ait-0\.0\.7-daemon-linux-x64\.tar\.gz/,
  );
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
if (args[0] === 'release' && args[1] === 'view' && args.includes('--json')) {
  console.log(fs.readdirSync(process.env.PUBLISHED_ASSETS).length);
}
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
  await rm(path.join(root, "release-assets", apk));
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
  assert.equal(build.if, "matrix.platform == 'mac'");
  assert.match(build.run, /--locked --release -p "\$\{\{ steps.layout.outputs.package \}\}"/);
  assert.match(build.run, /--bin "\$\{\{ steps.layout.outputs.binary \}\}" --target/);
  const linux = workflow.jobs.build.steps.find(
    (step) => step.name === "Build release daemon only (glibc 2.28 baseline)",
  );
  assert.equal(linux.if, "matrix.platform == 'linux'");
  assert.match(linux.run, /\.tmp\/release-tools\/scripts\/build-linux-daemon\.sh/);
  // The Linux container must use the same pinned toolchain as the macOS runner.
  const toolchain = workflow.jobs.build.steps.find((step) =>
    step.uses?.startsWith("dtolnay/rust-toolchain@"),
  );
  assert(linux.run.endsWith(` ${toolchain.uses.split("@")[1]}`));
  const script = await readFile(new URL("./build-linux-daemon.sh", import.meta.url), "utf8");
  assert.match(script, /--locked --release -p "\$PACKAGE" --bin "\$BINARY" --target "\$TARGET"/);
  assert.match(script, /^max_glibc=2\.28$/m);
  const collect = workflow.jobs.build.steps.find(
    (step) => step.name === "Collect installers and updater assets",
  );
  assert.match(collect.run, /steps.layout.outputs.desktop/);
  const names = workflow.jobs.build.steps.map((step) => step.name);
  const sign = workflow.jobs.build.steps.find(
    (step) => step.name === "Sign and notarize standalone macOS daemon",
  );
  assert.equal(sign.if, "matrix.platform == 'mac'");
  assert.match(sign.run, /codesign --force --timestamp --options runtime/);
  assert.match(sign.run, /notarytool submit/);
  const pack = workflow.jobs.build.steps.find((step) => step.name === "Package standalone daemon");
  assert.match(pack.run, /release-assets\.mjs daemon/);
  assert(names.indexOf(sign.name) < names.indexOf(pack.name));
  const upload = workflow.jobs.build.steps.findIndex((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  assert(names.indexOf(pack.name) < upload);
  const verify = workflow.jobs.release.steps.find(
    (step) => step.name === "Verify assets and write checksums",
  );
  assert.match(verify.run, /args=\(--daemon --windows\)/);
  const config = parse(
    await readFile(new URL("../apps/desktop/electron-builder.yml", import.meta.url), "utf8"),
  );
  assert.equal(config.appId, "dev.ait.desktop");
  assert.deepEqual(
    config.extraResources.filter((entry) => entry.to.startsWith("bin")),
    [],
  );
  for (const platform of ["mac", "linux"]) {
    assert.deepEqual(config[platform].extraResources, [
      { from: "release-resources/daemon/daemon", to: "bin/daemon" },
    ]);
  }
  assert.deepEqual(config.mac.binaries, ["Contents/Resources/bin/daemon"]);
  assert.deepEqual(config.win.target, ["nsis", "zip"]);
  assert.equal(config.win.extraResources, undefined);
  assert.deepEqual(config.linux.target, ["AppImage", "tar.gz", "deb", "rpm"]);
  for (const format of ["deb", "rpm"]) {
    assert.equal(config[format].packageName, "ait-desktop");
    assert.equal(config[format].afterInstall, "scripts/linux-sandbox/after-install.tpl");
    assert.equal(config[format].afterRemove, "scripts/linux-sandbox/after-remove.tpl");
    for (const library of [/asound|alsa/, /gbm/])
      assert(config[format].depends.some((name) => library.test(name)));
  }
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
  await verifyReleaseAssets({
    version: linux.version,
    directory: linux.destination,
  });
  assert.match(
    await readFile(path.join(linux.destination, "SHA256SUMS"), "utf8"),
    /  BUILD-INFO\.json/,
  );
  await writeFile(file, JSON.stringify({ ...info, version: "0.0.6" }));
  await assert.rejects(
    verifyReleaseAssets({
      version: linux.version,
      directory: linux.destination,
    }),
    /version differs/,
  );
  await writeFile(file, JSON.stringify({ ...info, sourceCommit: "main" }));
  await assert.rejects(
    verifyReleaseAssets({
      version: linux.version,
      directory: linux.destination,
    }),
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
