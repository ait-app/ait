const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { mkdtempSync, readFileSync, rmSync, symlinkSync } = require("node:fs");
const { tmpdir } = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { sourceVersion, packagedVersion } = require("./source-version.cjs");
const { valid } = require("semver");
const { getConfig } = require("app-builder-lib/out/util/config/config.js");

const root = path.resolve(__dirname, "..");
const directory = mkdtempSync(path.join(tmpdir(), "ait-build-version-"));
const archive = mkdtempSync(path.join(tmpdir(), "ait-build-archive-"));
test.after(() => rmSync(directory, { recursive: true, force: true }));
test.after(() => rmSync(archive, { recursive: true, force: true }));
const git = (...args) =>
  execFileSync("git", ["-C", directory, ...args], { encoding: "utf8" }).trim();
git("init", "-q");
execFileSync(
  "git",
  [
    "-C",
    directory,
    "-c",
    "user.name=Ait test",
    "-c",
    "user.email=test@example.invalid",
    "commit",
    "--allow-empty",
    "-qm",
    "fixture",
  ],
  {
    env: {
      ...process.env,
      GIT_AUTHOR_DATE: "2026-10-08T00:15:00Z",
      GIT_COMMITTER_DATE: "2026-10-08T00:15:00Z",
    },
  },
);
git("tag", "v0.1.12");
const hash = git("rev-parse", "--short=8", "HEAD");
const expected = `git-2026-10-08-${hash}`;
symlinkSync(directory, path.join(directory, "ait"));

function withBuildEnvironment(values, run) {
  const names = ["AIT_BUILD_VERSION", "RELEASE_TAG"];
  const original = Object.fromEntries(names.map((name) => [name, process.env[name]]));
  try {
    for (const name of names) {
      if (values[name] === undefined) delete process.env[name];
      else process.env[name] = values[name];
    }
    return run();
  } finally {
    for (const name of names) {
      if (original[name] === undefined) delete process.env[name];
      else process.env[name] = original[name];
    }
  }
}

test("Git installs use source date and hash even on a release tag", () => {
  withBuildEnvironment({}, () => assert.equal(sourceVersion("0.1.12", directory), expected));
});

test("explicit nightly and release builds retain their supplied identity", () => {
  withBuildEnvironment({ AIT_BUILD_VERSION: "2026-10-25-01234567", RELEASE_TAG: "v0.1.12" }, () =>
    assert.equal(sourceVersion("0.1.12", directory), "2026-10-25-01234567"),
  );
  withBuildEnvironment({ RELEASE_TAG: "v0.1.12" }, () =>
    assert.equal(sourceVersion("0.1.12", directory), "0.1.12"),
  );
});

test("missing Git metadata cannot silently inherit the release version", () => {
  withBuildEnvironment({}, () =>
    assert.throws(() => sourceVersion("0.1.12", archive), /Cannot identify source build/),
  );
  withBuildEnvironment({ RELEASE_TAG: "v0.1.12" }, () =>
    assert.equal(sourceVersion("0.1.12", archive), "0.1.12"),
  );
});

test("date-based package versions are valid SemVer, including numeric hashes", () => {
  for (const version of [
    "2026-01-02-01234567",
    "git-2026-01-02-01234567",
    "0.1.12",
    "0.1.12-beta.1",
  ]) {
    assert.equal(valid(packagedVersion(version)), packagedVersion(version));
  }
  assert.equal(packagedVersion("git-2026-01-02-01234567"), "2026.1.2-git.g01234567");
});

test("nightly CLI uses the UTC build day instead of the source commit day", () => {
  const output = path.join(directory, "nightly-output");
  const before = new Date().toISOString().slice(0, 10);
  execFileSync(process.execPath, [path.join(root, "scripts/nightly-build.mjs"), "identity"], {
    cwd: directory,
    env: { ...process.env, GITHUB_SHA: git("rev-parse", "HEAD"), GITHUB_OUTPUT: output },
  });
  const after = new Date().toISOString().slice(0, 10);
  assert.ok(
    [before, after].some((date) => readFileSync(output, "utf8") === `label=${hash}-${date}\n`),
  );
});

test("Git package recipes use date/hash versions and propagate the Git display identity", () => {
  const aur = execFileSync(
    "bash",
    [
      "-c",
      'source "$1"; srcdir="$2"; pkgver; printf "\\n"; _build_env; printf "%s\\n" "$AIT_BUILD_VERSION"',
      "bash",
      path.join(root, "packaging/aur/ait-git/PKGBUILD"),
      directory,
    ],
    { encoding: "utf8" },
  );
  assert.equal(aur, `20261008.g${hash}\n${expected}\n`);
  const local = execFileSync(
    "bash",
    ["-c", 'source "$1"; printf "%s\\n" "$pkgver"', "bash", path.join(root, "PKGBUILD")],
    {
      env: { ...process.env, AIT_MAKEPKG_DIR: path.join(directory, "makepkg") },
      encoding: "utf8",
    },
  );
  assert.match(local.trim(), /^\d{8}\.g[0-9a-f]{8}$/);
});

test("desktop builder loads date-based package and display metadata", async () => {
  const config = await getConfig(path.join(root, "apps/desktop"), "electron-builder.yml");
  const { version } = require("../apps/desktop/package.json");
  const identity = sourceVersion(version, root);
  assert.equal(config.extraMetadata.displayVersion, identity);
  assert.equal(config.extraMetadata.version, packagedVersion(identity));
  assert.equal(config.extraMetadata.name, "@ait/desktop");
});

test("Rust build identity agrees with the desktop for Git, nightly and releases", () => {
  const binary = path.join(directory, "build-version");
  execFileSync(
    "rustc",
    ["--edition=2024", path.join(root, "crates/model/build.rs"), "-o", binary],
    {
      env: { ...process.env, CARGO_PKG_VERSION: "0.1.12" },
    },
  );
  for (const [values, version] of [
    [{}, expected],
    [{ AIT_BUILD_VERSION: "2026-10-25-01234567" }, "2026-10-25-01234567"],
    [{ RELEASE_TAG: "v0.1.12" }, "0.1.12"],
  ]) {
    withBuildEnvironment(values, () => {
      const output = execFileSync(binary, { cwd: directory, encoding: "utf8" });
      assert.ok(output.includes(`cargo:rustc-env=AIT_BUILD_VERSION=${version}\n`), output);
      assert.equal(sourceVersion("0.1.12", directory), version);
    });
  }
  withBuildEnvironment({}, () =>
    assert.throws(
      () => execFileSync(binary, { cwd: archive, stdio: "pipe" }),
      /Cannot identify source build/,
    ),
  );
});
