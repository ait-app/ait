import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
  existsSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import yaml from "yaml";

const root = resolve(import.meta.dirname, "..");
const packages = ["ait-bin", "aitd-bin", "ait-git", "aitd-git"];
const git = spawnSync("which", ["git"], { encoding: "utf8" }).stdout.trim();
function command(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { encoding: "utf8", ...options });
  assert.equal(result.status, 0, result.stderr + result.stdout);
  return result.stdout;
}
function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), "aur publishing "));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  for (const part of ["repo/scripts", "bin", "remotes"])
    mkdirSync(join(dir, part), { recursive: true });
  cpSync(join(root, "packaging"), join(dir, "repo/packaging"), { recursive: true });
  cpSync(join(root, "LICENSE"), join(dir, "repo/LICENSE"));
  cpSync(join(root, "scripts/aur-publish.sh"), join(dir, "repo/scripts/aur-publish.sh"));
  const env = {
    ...process.env,
    PATH: `${join(dir, "bin")}:${process.env.PATH}`,
    FIXTURE: dir,
    REAL_GIT: git,
    AUR_PUSH: "0",
    AUR_GIT_NAME: "Fixture Maintainer",
    AUR_GIT_EMAIL: "fixture@example.invalid",
    NEXT_VERSION: "20261125.g12345678",
  };
  const stub = (name, body) => writeFileSync(join(dir, "bin", name), body, { mode: 0o755 });
  stub(
    "git",
    `#!/bin/bash
set -eu
if [[ "$1" = clone ]]; then
  [[ "\${FAIL_CLONE:-0}" != 1 ]] || exit 128
  args=("$@")
  pkg=\${args[2]##*/}
  exec "$REAL_GIT" clone --quiet "$FIXTURE/remotes/$pkg" "\${args[3]}"
fi
if [[ " $* " == *" push "* ]]; then echo pushed >> "$FIXTURE/actions"; exit; fi
exec "$REAL_GIT" "$@"
`,
  );
  stub(
    "curl",
    `#!/bin/bash
[[ "\${FAIL_DOWNLOAD:-0}" != 1 ]] || exit 22
[[ "\${BAD_MANIFEST:-0}" != 1 ]] || { echo 'invalid checksum'; exit; }
printf '%064d  Ait-linux-x86_64.AppImage\\n' 1
printf '%064d  Ait-0.2.27-daemon-linux-x64.tar.gz\\n' 2
printf '%064d  Ait-0.2.28-daemon-linux-x64.tar.gz\\n' 3
`,
  );
  stub(
    "vercmp",
    `#!/usr/bin/env node
const [a,b] = process.argv.slice(2);
console.log(Math.sign(a.localeCompare(b, 'en', {numeric:true})));
`,
  );
  stub("namcap", "#!/bin/bash\nexit 0\n");
  stub(
    "makepkg",
    `#!/bin/bash
set -eu
source ./PKGBUILD
case " $* " in
  *" --printsrcinfo "*)
    printf 'pkgbase = %s\\n\\tpkgver = %s\\n\\tpkgrel = %s\\npkgname = %s\\n' "$pkgname" "$pkgver" "$pkgrel" "$pkgname" ;;
  *" --nobuild "*)
    echo "fetch $pkgname $*" >> "$FIXTURE/actions"
    if [[ "$pkgver" != "$NEXT_VERSION" ]]; then
      sed -i "s/^pkgver=.*/pkgver=$NEXT_VERSION/;s/^pkgrel=.*/pkgrel=1/" PKGBUILD
    fi ;;
  *)
    echo "check $pkgname $*" >> "$FIXTURE/actions"
    [[ "\${FAIL_CHECKSUM:-0}" != 1 ]] || { echo 'checksum failed' >&2; exit 1; }
    if [[ " $* " == *" --syncdeps "* ]]; then
      mkdir -p "$PKGDEST"
      archive="$PKGDEST/$pkgname-$pkgver-$pkgrel.pkg.tar.zst"
      if [[ -f "$archive" && " $* " != *" --force "* ]]; then exit 13; fi
      touch "$archive"
    fi ;;
esac
`,
  );
  for (const pkg of packages)
    command(git, ["init", "--bare", "--quiet", join(dir, "remotes", `${pkg}.git`)]);
  return {
    dir,
    run(pkg, options = {}) {
      const work = join(dir, `work-${pkg}`);
      const result = spawnSync(
        "bash",
        [
          join(dir, "repo/scripts/aur-publish.sh"),
          options.stage ?? "publish",
          pkg,
          options.version ?? "0.2.27",
        ],
        {
          encoding: "utf8",
          env: { ...env, AUR_WORKDIR: work, ...options.env },
        },
      );
      return {
        ...result,
        work,
        candidate: join(work, "candidate"),
        output: result.stdout + result.stderr,
      };
    },
    accept(result, pkg) {
      command(git, [
        "-C",
        join(result.work, "aur"),
        "push",
        join(dir, "remotes", `${pkg}.git`),
        "HEAD:master",
      ]);
    },
    actions() {
      return existsSync(join(dir, "actions")) ? readFileSync(join(dir, "actions"), "utf8") : "";
    },
  };
}
const content = (result) => readFileSync(join(result.candidate, "PKGBUILD"), "utf8");

for (const pkg of packages) {
  test(`${pkg}: empty repository, candidate, no default push, and repeated publication`, (t) => {
    const f = fixture(t);
    const first = f.run(pkg);
    assert.equal(first.status, 0, first.output);
    assert.ok(existsSync(join(first.candidate, ".SRCINFO")));
    assert.doesNotMatch(f.actions(), /pushed|--syncdeps/);
    assert.equal(existsSync(join(first.candidate, "aitd.service")), pkg.startsWith("aitd-"));
    if (pkg.endsWith("-bin")) assert.match(f.actions(), /--verifysource/);
    else assert.match(f.actions(), /--nobuild --noprepare --nodeps/);
    f.accept(first, pkg);
    const before = f.actions();
    const again = f.run(pkg, { env: { NEXT_VERSION: "20261126.g87654321", AUR_PUSH: "1" } });
    assert.equal(again.status, 0, again.output);
    assert.match(again.output, /up to date/);
    assert.equal(f.actions(), before, "unchanged Git recipes do not fetch new upstream commits");
  });
}

test("same-version recipe updates increment pkgrel and explicit push is honored", (t) => {
  const f = fixture(t);
  for (const pkg of packages) {
    const first = f.run(pkg);
    f.accept(first, pkg);
    const path = join(f.dir, "repo/packaging/aur", pkg, "PKGBUILD");
    writeFileSync(path, `${readFileSync(path, "utf8")}\n# recipe update\n`);
    const update = f.run(pkg, { env: { AUR_PUSH: "1" } });
    assert.equal(update.status, 0, update.output);
    assert.match(content(update), /^pkgrel=2$/m);
  }
  assert.equal(f.actions().match(/pushed/g).length, 4);
});

test("stable upgrades reset pkgrel and downgrades are skipped", (t) => {
  const f = fixture(t);
  const first = f.run("aitd-bin");
  f.accept(first, "aitd-bin");
  const newer = f.run("aitd-bin", { version: "0.2.28" });
  assert.equal(newer.status, 0, newer.output);
  assert.match(content(newer), /^pkgver=0.2.28\npkgrel=1$/m);
  f.accept(newer, "aitd-bin");
  const older = f.run("aitd-bin");
  assert.equal(older.status, 0, older.output);
  assert.match(older.output, /newer 0.2.28/);
});

test("network failures, invalid manifests and asset checksum failures cannot publish", (t) => {
  for (const failure of ["FAIL_CLONE", "FAIL_DOWNLOAD", "BAD_MANIFEST", "FAIL_CHECKSUM"]) {
    const f = fixture(t);
    const result = f.run("aitd-bin", { env: { [failure]: "1", AUR_PUSH: "1" } });
    assert.notEqual(result.status, 0, failure);
    assert.doesNotMatch(f.actions(), /pushed/);
  }
});

test("builds run even for an unchanged recipe and emit matching package metadata", (t) => {
  const f = fixture(t);
  for (const pkg of packages) {
    const first = f.run(pkg);
    f.accept(first, pkg);
    const baseline = f.run(pkg, { stage: "build" });
    assert.equal(baseline.status, 0, baseline.output);
    assert.match(baseline.output, /Built/);
    const path = join(f.dir, "repo/packaging/aur", pkg, "PKGBUILD");
    writeFileSync(path, `${readFileSync(path, "utf8")}\n# rebuilt recipe\n`);
    const build = f.run(pkg, { stage: "build" });
    assert.equal(build.status, 0, build.output);
    const version = pkg.endsWith("-git") ? "20261125.g12345678" : "0.2.27";
    assert.ok(existsSync(join(build.work, "packages", `${pkg}-${version}-2.pkg.tar.zst`)));
    assert.match(readFileSync(join(build.candidate, ".SRCINFO"), "utf8"), /pkgrel = 2/);
    const unchanged = f.run(pkg, { stage: "build" });
    assert.equal(unchanged.status, 0, unchanged.output);
  }
  assert.doesNotMatch(f.actions(), /pushed/);
});

test("shared service changes update both daemon recipes and their checksums", (t) => {
  const f = fixture(t);
  for (const pkg of ["aitd-bin", "aitd-git"]) f.accept(f.run(pkg), pkg);
  const service = join(f.dir, "repo/packaging/systemd/aitd.service");
  writeFileSync(service, `${readFileSync(service, "utf8")}\n# service update\n`);
  for (const pkg of ["aitd-bin", "aitd-git"]) {
    const update = f.run(pkg);
    assert.equal(update.status, 0, update.output);
    assert.match(content(update), /^pkgrel=2$/m);
    command(
      "bash",
      ["-c", 'source ./PKGBUILD; printf "%s  aitd.service\\n" "${sha256sums[1]}" | sha256sum -c -'],
      { cwd: update.candidate },
    );
  }
});

test("daemon recipes contain only command, license and service; Git versions use eight hex digits", () => {
  for (const pkg of ["aitd-bin", "aitd-git"]) {
    const recipe = readFileSync(join(root, "packaging/aur", pkg, "PKGBUILD"), "utf8");
    assert.match(recipe, /provides=\("aitd=\$\{pkgver\}"\)/);
    assert.match(recipe, /conflicts=\('aitd'\)/);
    assert.doesNotMatch(recipe, /npm|electron|\.install|systemctl/);
    command("bash", ["-n", join(root, "packaging/aur", pkg, "PKGBUILD")]);
  }
  const service = readFileSync(join(root, "packaging/systemd/aitd.service"), "utf8");
  for (const setting of [
    "Type=exec",
    "ExecStart=/usr/bin/aitd run",
    "KillMode=mixed",
    "TimeoutStopSec=60s",
    "EnvironmentFile=-%h/.config/ait/daemon.env",
  ])
    assert.ok(service.includes(setting));
});

test("workflow resolves one version and isolates binary builds from AUR credentials", () => {
  const workflow = yaml.parse(
    readFileSync(join(root, ".github/workflows/publish-aur.yml"), "utf8"),
  );
  assert.deepEqual(workflow.on.workflow_dispatch.inputs.package.options, [
    "all",
    "ait-bin",
    "aitd-bin",
  ]);
  const { release, build, publish } = workflow.jobs;
  assert.match(release.steps[0].run, /gh release view/);
  assert.equal(build.needs, "release");
  assert.deepEqual(publish.needs, ["release", "build"]);
  assert.equal(publish.environment, "aur");
  assert.doesNotMatch(JSON.stringify(build), /secrets\.|AUR_SSH_PRIVATE_KEY/);
  assert.match(JSON.stringify(build), /needs.release.outputs.version/);
  assert.match(JSON.stringify(publish), /needs.release.outputs.version/);
  assert.doesNotMatch(JSON.stringify([build, publish]), /aitd?-git/);
});

test("both Git recipes derive identical date/hash and display versions from the checkout", (t) => {
  const f = fixture(t);
  const src = join(f.dir, "ait");
  command(git, ["init", "--quiet", src]);
  command(
    git,
    [
      "-C",
      src,
      "-c",
      "user.name=Fixture",
      "-c",
      "user.email=fixture@example.invalid",
      "commit",
      "--quiet",
      "--allow-empty",
      "-m",
      "fixture",
    ],
    {
      env: {
        ...process.env,
        GIT_AUTHOR_DATE: "2026-10-08T12:00:00Z",
        GIT_COMMITTER_DATE: "2026-10-08T12:00:00Z",
      },
    },
  );
  const hash = command(git, ["-C", src, "rev-parse", "--short=8", "HEAD"]).trim();
  for (const pkg of ["ait-git", "aitd-git"]) {
    const output = command("bash", [
      "-c",
      'source "$1"; srcdir="$2"; pkgver; printf "\\n"; _build_env; printf "%s\\n" "$AIT_BUILD_VERSION"',
      "bash",
      join(root, "packaging/aur", pkg, "PKGBUILD"),
      f.dir,
    ]);
    assert.equal(output, `20261008.g${hash}\ngit-2026-10-08-${hash}\n`);
  }
});
