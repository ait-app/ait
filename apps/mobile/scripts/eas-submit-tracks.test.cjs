const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const yaml = require("yaml");

const appDir = `${__dirname}/..`;
const easConfig = JSON.parse(fs.readFileSync(`${appDir}/eas.json`, "utf8"));
const workflowPath = `${appDir}/.eas/workflows/release-mobile.yml`;

test("internal submit profile cannot change the production release path", () => {
  assert.deepEqual(easConfig.submit.ait.android, {
    track: "internal",
    releaseStatus: "completed",
  });
  assert.deepEqual(easConfig.submit.production.android, {
    track: "production",
    releaseStatus: "completed",
  });
  assert.deepEqual(easConfig.submit.ait.ios, {
    ascAppId: "6817043477",
    appleTeamId: "SVS7GV79T9",
    bundleIdentifier: "com.necokeine.ait",
  });
  assert.equal(easConfig.build.ait.extends, "production");
  assert.equal(easConfig.build.ait.env.EAS_PROJECT_ID, "379ada50-82c0-4d4a-bac9-cb8c113cf38d");

  const workflow = yaml.parse(fs.readFileSync(workflowPath, "utf8"));
  const triggers = workflow.on ?? workflow[true];
  assert.equal(triggers.push, undefined);
  assert.deepEqual(triggers.workflow_dispatch, {});

  for (const job of Object.values(workflow.jobs)) {
    if (job.type === "build" || job.type === "submit") {
      assert.equal(job.params.profile, "production");
    }
    if (job.params?.profile) {
      assert.notEqual(job.params.profile, "ait");
    }
  }
});
