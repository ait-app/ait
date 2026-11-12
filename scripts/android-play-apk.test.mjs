import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createVerify, generateKeyPairSync } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { parse } from "yaml";
import {
  createServiceAccountAssertion,
  downloadUniversalApk,
  normalizeCertificateHash,
  requestAccessToken,
  verifyGoogleSignedApk,
} from "./android-play-apk.mjs";

const workflow = parse(
  readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
);
const play = workflow.jobs["android-play"];
const attach = workflow.jobs["android-apk"];
const digest = "ab".repeat(32);
const plan = { version: "1.2.3", versionCode: 840501, packageId: "dev.ait.mobile" };
const generatedUrl =
  "https://androidpublisher.googleapis.com/androidpublisher/v3/applications/dev.ait.mobile/generatedApks/840501";
const universal = {
  generatedApks: [
    { certificateSha256Hash: digest, generatedUniversalApk: { downloadId: "id/1+2" } },
  ],
};
const json = (body, status = 200) => new Response(JSON.stringify(body), { status });

function tempDir(t) {
  const directory = mkdtempSync(path.join(tmpdir(), "ait-play-apk-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test("service account assertion is an RS256 JWT for the Android Publisher scope", () => {
  const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
  const account = {
    client_email: "ci@example.iam.gserviceaccount.com",
    private_key: privateKey.export({ type: "pkcs8", format: "pem" }),
  };
  const [header, payload, signature] = createServiceAccountAssertion(account, 1e12).split(".");
  const decode = (part) => JSON.parse(Buffer.from(part, "base64url").toString());
  assert.deepEqual(decode(header), { alg: "RS256", typ: "JWT" });
  const claims = decode(payload);
  assert.equal(claims.iss, account.client_email);
  assert.equal(claims.scope, "https://www.googleapis.com/auth/androidpublisher");
  assert.equal(claims.aud, "https://oauth2.googleapis.com/token");
  assert.equal(claims.exp - claims.iat, 3600);
  const verifier = createVerify("RSA-SHA256").update(`${header}.${payload}`);
  assert(verifier.verify(publicKey, Buffer.from(signature, "base64url")));
  assert.throws(() => createServiceAccountAssertion({}), /service account/);
});

test("access token exchange posts a JWT bearer grant and fails without leaking it", async () => {
  const { privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
  const account = {
    client_email: "ci@example.com",
    private_key: privateKey.export({ type: "pkcs8", format: "pem" }),
  };
  let request;
  const token = await requestAccessToken(account, async (url, init) => {
    request = { url, body: new URLSearchParams(init.body) };
    return json({ access_token: "token" });
  });
  assert.equal(token, "token");
  assert.equal(request.url, "https://oauth2.googleapis.com/token");
  assert.equal(request.body.get("grant_type"), "urn:ietf:params:oauth:grant-type:jwt-bearer");
  await assert.rejects(
    requestAccessToken(account, async () => json({ error: "invalid_grant" }, 400)),
    (error) => /returned 400/.test(error.message) && !error.message.includes("BEGIN"),
  );
});

test("certificate hashes normalize from hex, colon hex or base64", () => {
  assert.equal(normalizeCertificateHash(digest.toUpperCase()), digest);
  assert.equal(normalizeCertificateHash(digest.match(/../g).join(":")), digest);
  assert.equal(normalizeCertificateHash(Buffer.from(digest, "hex").toString("base64")), digest);
  assert.throws(() => normalizeCertificateHash("abc"), /Unrecognized/);
});

test("waits for Play to generate the universal APK and streams that exact download", async (t) => {
  const output = path.join(tempDir(t), "app.apk");
  const calls = [];
  const replies = [
    json({ error: { message: "processing" } }, 404),
    json({ generatedApks: [] }),
    json(universal),
  ];
  const sleeps = [];
  const hash = await downloadUniversalApk({
    packageName: "dev.ait.mobile",
    versionCode: 840501,
    accessToken: "token",
    output,
    sleep: async (ms) => sleeps.push(ms),
    attempts: 3,
    fetch: async (url, init) => {
      calls.push(url);
      assert.equal(init.headers.Authorization, "Bearer token");
      return url === generatedUrl ? replies.shift() : new Response("apk bytes");
    },
  });
  assert.equal(hash, digest);
  assert.deepEqual(sleeps, [30_000, 30_000]);
  assert.equal(calls.at(-1), `${generatedUrl}/downloads/id%2F1%2B2:download?alt=media`);
  assert.equal(readFileSync(output, "utf8"), "apk bytes");
});

test("download stops on permission errors, ambiguous keys or a missing APK", async (t) => {
  const output = path.join(tempDir(t), "app.apk");
  const attempt = (reply, attempts = 2) => {
    const calls = [];
    const promise = downloadUniversalApk({
      packageName: "dev.ait.mobile",
      versionCode: 840501,
      accessToken: "token",
      output,
      sleep: async () => {},
      attempts,
      fetch: async (url) => (calls.push(url), reply()),
    });
    return { calls, promise };
  };
  const denied = attempt(() => json({ error: { message: "No permission" } }, 403));
  await assert.rejects(denied.promise, /returned 403: No permission/);
  assert.equal(denied.calls.length, 1);
  const twoKeys = attempt(() =>
    json({ generatedApks: [universal.generatedApks[0], universal.generatedApks[0]] }),
  );
  await assert.rejects(twoKeys.promise, /more than one signing key/);
  const missing = attempt(() => json({ generatedApks: [{ certificateSha256Hash: digest }] }));
  await assert.rejects(missing.promise, /no universal APK for versionCode 840501/);
  assert(missing.calls.every((url) => url === generatedUrl));
});

test("the APK must match the Play build and Google's signing certificate", () => {
  const badging = `package: name='dev.ait.mobile' versionCode='840501' versionName='1.2.3'
native-code: 'arm64-v8a' 'armeabi-v7a' 'x86' 'x86_64'
`;
  const certificates = `Signer #1 certificate SHA-256 digest: ${digest}\n`;
  verifyGoogleSignedApk({ badging, certificates, plan, certificateSha256: digest });
  // v3.1 key rotation (what Play produced for 0.2.8): one signer per SDK range plus a source stamp.
  const rotated = `Signer (minSdkVersion=33, maxSdkVersion=2147483647) certificate DN: CN=Android
Signer (minSdkVersion=33, maxSdkVersion=2147483647) certificate SHA-256 digest: ${"ef".repeat(32)}
Signer (minSdkVersion=24, maxSdkVersion=32) certificate SHA-256 digest: ${digest}
Source Stamp Signer certificate SHA-256 digest: ${"12".repeat(32)}
`;
  for (const certificateSha256 of [digest, "ef".repeat(32)])
    assert.deepEqual(
      verifyGoogleSignedApk({ badging, certificates: rotated, plan, certificateSha256 }).signers,
      ["ef".repeat(32), digest],
    );
  assert.throws(
    () =>
      verifyGoogleSignedApk({
        badging,
        certificates: rotated,
        plan,
        certificateSha256: "12".repeat(32),
      }),
    /not among the APK signers/,
  );
  assert.throws(
    () =>
      verifyGoogleSignedApk({
        badging,
        certificates: "Verifies\n",
        plan,
        certificateSha256: digest,
      }),
    /no signing certificate/,
  );
  assert.throws(
    () =>
      verifyGoogleSignedApk({ badging, certificates, plan, certificateSha256: "cd".repeat(32) }),
    /not among the APK signers/,
  );
  assert.throws(() =>
    verifyGoogleSignedApk({
      badging: badging.replace("840501", "840502"),
      certificates,
      plan,
      certificateSha256: digest,
    }),
  );
});

test("Play job exposes its submission so the APK job can follow it", () => {
  assert.deepEqual(play.outputs, {
    version: "${{ steps.plan.outputs.version }}",
    version_code: "${{ steps.plan.outputs.version_code }}",
    submitted: "${{ steps.submit.outcome }}",
  });
  assert.equal(
    play.steps.find((step) => step.id === "submit").name,
    "Submit this verified AAB to Google Play",
  );
  assert(!JSON.stringify(play).includes("android-play-apk"));
});

test("APK job downloads from Play and only writes to the draft release", () => {
  assert.deepEqual(attach.needs, ["android-play", "release"]);
  assert.equal(attach.if, "needs.android-play.outputs.submitted == 'success'");
  assert.equal(attach["continue-on-error"], true);
  assert.deepEqual(attach.permissions, { contents: "write" });
  assert(!workflow.jobs.release.needs.includes("android-apk"));
  const checkout = attach.steps.find((step) => step.uses?.startsWith("actions/checkout"));
  assert.equal(checkout.with.ref, "${{ github.workflow_sha }}");
  assert.equal(checkout.with["persist-credentials"], false);
  const names = attach.steps.map((step) => step.name);
  assert(
    names.indexOf("Download Google-signed universal APK") <
      names.indexOf("Attach APK and refresh checksums"),
  );
  const download = attach.steps.find(
    (step) => step.name === "Download Google-signed universal APK",
  );
  assert.deepEqual(Object.keys(download.env), ["PLAY_SERVICE_ACCOUNT_JSON"]);
  for (const step of attach.steps.filter((step) => step.run)) {
    assert.doesNotMatch(step.run, /\$\{\{/);
    const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
    assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
  }
  assert.match(download.run, /--version-code "\$VERSION_CODE"/);
  assert.match(download.run, /grep -v -- -rc/);
});

function runAttach(t, { draft, sums }) {
  const directory = tempDir(t);
  const step = attach.steps.find(
    (candidate) => candidate.name === "Attach APK and refresh checksums",
  );
  mkdirSync(path.join(directory, "apk"));
  writeFileSync(path.join(directory, "apk", "Ait-1.2.3-android.apk"), "google apk");
  const log = path.join(directory, "calls.jsonl");
  const gh = path.join(directory, "gh");
  writeFileSync(
    gh,
    `#!/usr/bin/env node
const fs = require("node:fs");
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify(args) + "\\n");
if (args[1] === "view") console.log(${JSON.stringify(String(draft))});
if (args[1] === "download") {
  const dir = args[args.indexOf("--dir") + 1];
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(dir + "/SHA256SUMS", ${JSON.stringify(sums)});
}
`,
  );
  chmodSync(gh, 0o755);
  const result = spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
    cwd: directory,
    encoding: "utf8",
    env: {
      PATH: `${directory}${path.delimiter}${process.env.PATH}`,
      RELEASE_TAG: "v1.2.3",
      GITHUB_REPOSITORY: "ait-app/ait",
    },
  });
  const calls = readFileSync(log, "utf8").trim().split("\n").map(JSON.parse);
  const output = path.join(directory, "SHA256SUMS");
  return { result, calls, sums: spawnSync("cat", [output], { encoding: "utf8" }).stdout };
}

test("attaching replaces the APK checksum line and keeps SHA256SUMS sorted", (t) => {
  const apkHash = spawnSync("bash", ["-c", "printf 'google apk' | sha256sum"], {
    encoding: "utf8",
  }).stdout.split(" ")[0];
  const { result, calls, sums } = runAttach(t, {
    draft: true,
    sums: `${"1".repeat(64)}  Ait-1.2.3-android.apk\n${"2".repeat(64)}  Ait-1.2.3-mac-arm64.dmg\n${"3".repeat(64)}  BUILD-INFO.json\n`,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    sums,
    `${apkHash}  Ait-1.2.3-android.apk\n${"2".repeat(64)}  Ait-1.2.3-mac-arm64.dmg\n${"3".repeat(64)}  BUILD-INFO.json\n`,
  );
  const upload = calls.find((args) => args[1] === "upload");
  assert.deepEqual(upload.slice(2, 5), ["v1.2.3", "apk/Ait-1.2.3-android.apk", "SHA256SUMS"]);
  assert(upload.includes("--clobber"));
});

test("a published release is left untouched", (t) => {
  const { result, calls } = runAttach(t, { draft: false, sums: "" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /already published/);
  assert(!calls.some((args) => ["download", "upload"].includes(args[1])));
});
