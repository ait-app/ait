const { execFileSync } = require("node:child_process");

function sourceVersion(releaseVersion, directory) {
  if (process.env.AIT_BUILD_VERSION) return process.env.AIT_BUILD_VERSION;
  if (process.env.RELEASE_TAG === `v${releaseVersion}`) return releaseVersion;
  const git = (...args) =>
    execFileSync("git", ["-C", directory, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  try {
    return `git-${git("show", "-s", "--format=%cs", "HEAD")}-${git("rev-parse", "--short=8", "HEAD")}`;
  } catch (cause) {
    throw new Error(
      "Cannot identify source build: provide Git metadata, AIT_BUILD_VERSION or RELEASE_TAG.",
      { cause },
    );
  }
}

function packagedVersion(version) {
  const build = /^(git-)?(\d{4})-(\d{2})-(\d{2})-([0-9a-f]{8})$/.exec(version);
  if (!build) return version;
  const [, git, year, month, day, hash] = build;
  return `${year}.${Number(month)}.${Number(day)}-${git ? "git." : ""}g${hash}`;
}

module.exports = { sourceVersion, packagedVersion };
