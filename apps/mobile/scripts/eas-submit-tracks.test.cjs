const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");

const appDir = `${__dirname}/..`;
const easConfig = JSON.parse(fs.readFileSync(`${appDir}/eas.json`, "utf8"));

test("legacy EAS profiles keep internal and production submission separate", () => {
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
});
