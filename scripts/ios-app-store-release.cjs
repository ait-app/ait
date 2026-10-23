const { execFileSync } = require("node:child_process");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { setTimeout: sleep } = require("node:timers/promises");

const workflowName = "Release iOS to App Store";
const cloudUrl =
  "https://appstoreconnect.apple.com/teams/3e9381fe-a5bd-4fda-bc82-4f218c20480b/xcode-cloud/products/9153f73a-c52d-458b-ab3c-762228945ce5";
const testFlightUrl =
  "https://appstoreconnect.apple.com/apps/6817043477/testflight/groups/909a6e53-ceb0-4f74-86e7-1b29802d7555";
const stableVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const actionNames = ["Build - iOS", "Archive - iOS"].map(
  (action) => `Ait | ${workflowName} | ${action}`,
);

function validateSource(source, tag = "") {
  // Read source as data. In particular, never execute app.config.js or an old tag's scripts.
  const read = (file) => readFileSync(join(source, file), "utf8");
  const pkg = JSON.parse(read("package.json"));
  const version = pkg.version;
  if (!stableVersion.test(version))
    throw new Error("App Store source must have a stable X.Y.Z version.");
  if (tag && tag !== `v${version}`) throw new Error(`Source tag must match v${version}.`);
  const cargo = read("Cargo.toml").match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/)?.[1];
  const cargoVersion = cargo?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const lock = JSON.parse(read("package-lock.json"));
  const versions = { Cargo: cargoVersion, lock: lock.version };
  for (const directory of ["", ...pkg.workspaces]) {
    versions[`${directory}/package.json`] = JSON.parse(
      read(join(directory, "package.json")),
    ).version;
    versions[`lock:${directory}`] = lock.packages[directory]?.version;
  }
  versions["iOS Info.plist"] = read("apps/mobile/ios/Ait/Info.plist").match(
    /<key>CFBundleShortVersionString<\/key>\s*<string>([^<]+)<\/string>/,
  )?.[1];
  for (const [name, actual] of Object.entries(versions)) {
    if (actual !== version) throw new Error(`${name} version ${actual} differs from ${version}.`);
  }
  return version;
}

function triggerTag(version, runId, attempt) {
  if (!stableVersion.test(version) || !/^[1-9]\d*$/.test(runId) || !/^[1-9]\d*$/.test(attempt)) {
    throw new Error("Invalid version, GitHub run ID or attempt for the trigger tag.");
  }
  return `ios-app-store/v${version}/${runId}-${attempt}`;
}

function buildUrl(check) {
  if (check.app?.slug !== "xcode-cloud" || !actionNames.includes(check.name)) return null;
  // Keep the build identity when Apple replaces a running check with a new completed check ID.
  return (
    check.details_url?.match(
      /^(https:\/\/appstoreconnect\.apple\.com\/teams\/[\da-f-]+\/apps\/6817043477\/ci\/builds\/[\da-f-]+)\/action\/[\da-f-]+$/i,
    )?.[1] ?? null
  );
}

function archiveState(checks, previousBuilds, sha) {
  const current = checks.filter(
    (check) => check.head_sha === sha && buildUrl(check) && !previousBuilds.has(buildUrl(check)),
  );
  const builds = new Set(current.map(buildUrl));
  if (builds.size > 1) {
    throw new Error(
      "Multiple new Cloud builds found for this commit. Check Cloud before retrying.",
    );
  }
  const url = [...builds][0];
  if (!url) return { status: "waiting" };
  const latest = actionNames.map(
    (name) => current.filter((check) => check.name === name).sort((a, b) => b.id - a.id)[0],
  );
  const failed = latest.find(
    (check) => check?.status === "completed" && check.conclusion !== "success",
  );
  if (failed) return { status: "failed", url, message: `${failed.name}: ${failed.conclusion}` };
  return {
    status: latest.every((check) => check?.status === "completed") ? "success" : "running",
    url,
  };
}

async function waitForArchive({
  listChecks,
  previousBuilds,
  sha,
  report,
  pause = sleep,
  now = Date.now,
}) {
  const started = now();
  let found = false;
  while (now() - started < 90 * 60_000) {
    const state = archiveState(await listChecks(), previousBuilds, sha);
    await report(state);
    if (state.status === "success") return state;
    if (state.status === "failed") throw new Error(`${state.message}: ${state.url}`);
    found ||= state.status === "running";
    if (!found && now() - started >= 15 * 60_000) {
      throw new Error(
        "No new Xcode Cloud build appeared within 15 minutes. Check the tag start condition and GitHub integration; the tag is retained. Do not blindly rerun.",
      );
    }
    await pause(30_000);
  }
  throw new Error(
    "Timed out waiting for the Cloud archive. The Cloud build may still be running; inspect it before retrying.",
  );
}

async function release({ github, context, core, source }) {
  const version = validateSource(source, process.env.RELEASE_TAG || "");
  const tag = triggerTag(version, String(context.runId), process.env.GITHUB_RUN_ATTEMPT);
  const git = (...args) =>
    execFileSync("git", ["-C", source, ...args], { encoding: "utf8" }).trim();
  const sha = git("rev-parse", "HEAD");
  if (!/^[\da-f]{40}$/.test(sha)) throw new Error("Invalid source commit.");
  const listChecks = () =>
    github.paginate(github.rest.checks.listForRef, {
      ...context.repo,
      ref: sha,
      filter: "all",
      per_page: 100,
    });
  const previousBuilds = new Set((await listChecks()).map(buildUrl).filter(Boolean));
  core.setOutput("source_commit", sha);
  core.setOutput("trigger_tag", tag);
  await core.summary
    .addHeading(workflowName)
    .addRaw(
      `Source: \`${sha}\` · version \`${version}\`\n\nTrigger tag: \`${tag}\`\n\n` +
        `[Xcode Cloud](${cloudUrl}/groups) · [Ait External](${testFlightUrl})\n\n` +
        "Cloud uploads the archive and submits it to Ait External. Beta review may still be required. Formal App Store review and release remain manual.\n\n" +
        "GitHub observes Build/Archive checks only; a green run does not confirm upload, Beta approval or tester availability. Cancelling this GitHub run does not cancel Cloud.\n\n",
    )
    .write();
  // A single immutable ref. Never force-update a release tag or reuse a previous attempt's ref.
  git("push", "origin", `${sha}:refs/tags/${tag}`);
  await core.summary.addRaw("Trigger tag pushed. Waiting for new Xcode Cloud checks.\n\n").write();
  let lastStatus;
  let reportedUrl;
  try {
    await waitForArchive({
      listChecks,
      previousBuilds,
      sha,
      report: async (state) => {
        if (state.status !== lastStatus) core.info(`Xcode Cloud: ${state.status}`);
        lastStatus = state.status;
        if (state.url && state.url !== reportedUrl) {
          reportedUrl = state.url;
          core.setOutput("cloud_build_url", state.url);
          await core.summary.addRaw(`[Cloud build](${state.url})\n\n`).write();
        }
      },
    });
    await core.summary
      .addRaw(
        "Build and Archive succeeded. Open the Cloud build to verify upload and the Ait External post-action; check TestFlight for processing/Beta review status.\n",
      )
      .write();
  } catch (error) {
    await core.summary
      .addRaw(
        "GitHub could not confirm a successful archive. Inspect Cloud before retrying: the trigger tag is retained and a remote build is not cancelled automatically.\n",
      )
      .write();
    throw error;
  }
}

module.exports = { validateSource, triggerTag, archiveState, waitForArchive, release };
