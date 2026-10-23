import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import yaml from "yaml";
import {
  archiveState,
  triggerTag,
  validateSource,
  waitForArchive,
} from "./ios-app-store-release.cjs";

const sha = "a".repeat(40);
const baseUrl =
  "https://appstoreconnect.apple.com/teams/3e9381fe-a5bd-4fda-bc82-4f218c20480b/apps/6817043477/ci/builds";
const oldUrl = `${baseUrl}/00000000-0000-0000-0000-000000000001`;
const newUrl = `${baseUrl}/00000000-0000-0000-0000-000000000002`;
function check(action, id, overrides = {}) {
  return {
    id,
    name: `Ait | Release iOS to App Store | ${action} - iOS`,
    app: { slug: "xcode-cloud" },
    head_sha: sha,
    details_url: `${newUrl}/action/00000000-0000-0000-0000-000000000003`,
    status: "completed",
    conclusion: "success",
    ...overrides,
  };
}

test("validates native and workspace versions without executing source code", () => {
  const root = mkdtempSync(join(tmpdir(), "ait-ios-release-"));
  const write = (path, content) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(
      join(root, path),
      typeof content === "string" ? content : JSON.stringify(content),
    );
  };
  try {
    write("package.json", { version: "0.1.15", workspaces: ["apps/mobile"] });
    write("apps/mobile/package.json", { version: "0.1.15" });
    write("Cargo.toml", '[workspace.package]\nversion = "0.1.15"\n[dependencies]\n');
    write("package-lock.json", {
      version: "0.1.15",
      packages: { "": { version: "0.1.15" }, "apps/mobile": { version: "0.1.15" } },
    });
    write(
      "apps/mobile/ios/Ait/Info.plist",
      "<key>CFBundleShortVersionString</key><string>0.1.15</string>",
    );
    write("apps/mobile/app.config.js", "throw new Error('must not execute source');");
    assert.equal(validateSource(root, "v0.1.15"), "0.1.15");
    assert.throws(() => validateSource(root, "v0.1.14"), /Source tag must match/);
    write(
      "apps/mobile/ios/Ait/Info.plist",
      "<key>CFBundleShortVersionString</key><string>0.1.13</string>",
    );
    assert.throws(() => validateSource(root), /iOS Info.plist version 0.1.13 differs/);
    write(
      "apps/mobile/ios/Ait/Info.plist",
      "<key>CFBundleShortVersionString</key><string>0.1.15</string>",
    );
    write("apps/mobile/package.json", { version: "0.1.14" });
    assert.throws(() => validateSource(root), /apps\/mobile\/package.json version/);
    write("package.json", { version: "0.1.15-beta.1", workspaces: [] });
    assert.throws(() => validateSource(root), /stable/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("each run and attempt gets a safe, distinct trigger ref", () => {
  assert.equal(triggerTag("0.1.15", "123", "1"), "ios-app-store/v0.1.15/123-1");
  assert.notEqual(triggerTag("0.1.15", "123", "2"), triggerTag("0.1.15", "123", "1"));
  assert.notEqual(triggerTag("0.1.15", "124", "1"), triggerTag("0.1.15", "123", "1"));
  for (const args of [
    ["0.1.15\n", "123", "1"],
    ["0.1.15", "../bad", "1"],
    ["0.1.15", "123", "0"],
  ]) {
    assert.throws(() => triggerTag(...args));
  }
});

test("old builds, unrelated apps and other source commits cannot satisfy the release", () => {
  const checks = [
    check("Archive", 99, { details_url: `${oldUrl}/action/abcd` }),
    check("Build", 100, { head_sha: "b".repeat(40) }),
    check("Archive", 101, { app: { slug: "other-app" } }),
    check("Archive", 102, { name: "Ait | Public TestFlight | Archive - iOS" }),
  ];
  assert.equal(archiveState(checks, new Set([oldUrl]), sha).status, "waiting");
});

test("replacement completed checks supersede running checks for the same Cloud action", () => {
  const checks = [
    check("Build", 1, { status: "in_progress", conclusion: null }),
    check("Archive", 2, { status: "in_progress", conclusion: null }),
    check("Build", 3),
    check("Archive", 4),
  ];
  assert.deepEqual(archiveState(checks, new Set(), sha), { status: "success", url: newUrl });
  assert.equal(archiveState(checks.slice(0, 3), new Set(), sha).status, "running");
});

test("failed, cancelled or skipped actions fail the workflow and ambiguous builds are rejected", () => {
  for (const conclusion of ["failure", "cancelled", "skipped", "neutral", "timed_out"]) {
    assert.equal(
      archiveState([check("Archive", 4, { conclusion })], new Set(), sha).status,
      "failed",
    );
  }
  assert.throws(
    () =>
      archiveState(
        [check("Build", 1), check("Archive", 2, { details_url: `${oldUrl}/action/abcd` })],
        new Set(),
        sha,
      ),
    /Multiple new Cloud builds/,
  );
});

test("waits for both actions, propagates failure, and bounds missing webhook/build waits", async () => {
  let time = 0;
  let reads = 0;
  const params = {
    previousBuilds: new Set(),
    sha,
    now: () => time,
    pause: async (ms) => {
      time += ms;
    },
    report: async () => {},
  };
  const success = await waitForArchive({
    ...params,
    listChecks: async () => {
      reads += 1;
      return reads === 1 ? [] : [check("Build", 1), check("Archive", 2)];
    },
  });
  assert.equal(success.status, "success");
  await assert.rejects(
    waitForArchive({
      ...params,
      listChecks: async () => [check("Build", 3, { conclusion: "failure" })],
    }),
    /failure/,
  );
  await assert.rejects(waitForArchive({ ...params, listChecks: async () => [] }), /15 minutes/);
  await assert.rejects(
    waitForArchive({
      ...params,
      listChecks: async () => [check("Archive", 4, { status: "in_progress", conclusion: null })],
    }),
    /Timed out/,
  );
});

test("release entry is manual and runs tooling from the workflow commit", () => {
  const workflow = yaml.parse(
    readFileSync(
      new URL("../.github/workflows/release-ios-app-store.yml", import.meta.url),
      "utf8",
    ),
  );
  assert.deepEqual(Object.keys(workflow.on), ["workflow_dispatch"]);
  assert.equal(workflow.name, "Release iOS to App Store");
  assert.equal(workflow.concurrency["cancel-in-progress"], false);
  const steps = workflow.jobs.release.steps;
  assert.equal(
    steps.find((step) => step.with?.path === "tooling").with.ref,
    "${{ github.workflow_sha }}",
  );
  assert.equal(workflow.jobs.release.permissions.contents, "write");
  assert.equal(workflow.jobs.release.permissions.checks, "read");
});
