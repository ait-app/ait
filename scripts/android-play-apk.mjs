import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createSign } from "node:crypto";
import { createWriteStream } from "node:fs";
import { mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { setTimeout as delay } from "node:timers/promises";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";
import { validateAndroidBadging } from "./android-release.mjs";

const API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications";
const SCOPE = "https://www.googleapis.com/auth/androidpublisher";
// Play generates APKs after processing the uploaded bundle; wait up to 15 minutes.
const LIST_ATTEMPTS = 30;
const LIST_INTERVAL_MS = 30_000;

export function createServiceAccountAssertion(account, now = Date.now()) {
  assert(account.client_email && account.private_key, "Invalid Play service account JSON");
  const iat = Math.floor(now / 1000);
  const encode = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
  const body = `${encode({ alg: "RS256", typ: "JWT" })}.${encode({
    iss: account.client_email,
    scope: SCOPE,
    aud: account.token_uri ?? "https://oauth2.googleapis.com/token",
    iat,
    exp: iat + 3600,
  })}`;
  const signature = createSign("RSA-SHA256").update(body).sign(account.private_key, "base64url");
  return `${body}.${signature}`;
}

export async function requestAccessToken(account, fetch) {
  const response = await fetch(account.token_uri ?? "https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
      assertion: createServiceAccountAssertion(account),
    }),
  });
  const body = await response.json().catch(() => null);
  assert(response.ok && body?.access_token, `Google OAuth returned ${response.status}`);
  return body.access_token;
}

// Google reports the certificate as hex (optionally colon-separated) or base64.
export function normalizeCertificateHash(value) {
  const hex = String(value).replaceAll(":", "").toLowerCase();
  if (/^[\da-f]{64}$/.test(hex)) return hex;
  const bytes = Buffer.from(String(value), "base64");
  assert.equal(bytes.length, 32, "Unrecognized Play signing certificate hash");
  return bytes.toString("hex");
}

export async function downloadUniversalApk({
  packageName,
  versionCode,
  accessToken,
  output,
  fetch,
  sleep = delay,
  attempts = LIST_ATTEMPTS,
}) {
  const base = `${API}/${packageName}/generatedApks/${versionCode}`;
  const headers = { Authorization: `Bearer ${accessToken}` };
  let generated;
  for (let attempt = 1; attempt <= attempts && !generated; attempt += 1) {
    const response = await fetch(base, { headers });
    const body = await response.json().catch(() => null);
    // 404 and 5xx mean the bundle is still processing or Play is briefly unavailable.
    const pending = response.status === 404 || response.status === 429 || response.status >= 500;
    if (!response.ok && !pending) {
      const detail = body?.error?.message ?? "no detail";
      throw new Error(`Play generatedApks returned ${response.status}: ${detail}`);
    }
    const keys = response.ok ? (body?.generatedApks ?? []) : [];
    assert(keys.length <= 1, "Play returned APKs for more than one signing key");
    if (keys[0]?.generatedUniversalApk?.downloadId) generated = keys[0];
    else if (attempt < attempts) await sleep(LIST_INTERVAL_MS);
  }
  assert(generated, `Play has no universal APK for versionCode ${versionCode}`);
  const id = encodeURIComponent(generated.generatedUniversalApk.downloadId);
  const response = await fetch(`${base}/downloads/${id}:download?alt=media`, { headers });
  assert(response.ok && response.body, `Play APK download returned ${response.status}`);
  await pipeline(Readable.fromWeb(response.body), createWriteStream(output));
  return normalizeCertificateHash(generated.certificateSha256Hash);
}

// apksigner's signer labels change between build-tools versions ("Signer #1",
// "Signer (minSdkVersion=…)", "V3.0 Signer:", "V3.2 Hybrid PQC Signer: (…)"), so read every
// certificate digest line and skip only Play's source stamp, which does not sign the app.
export function apkSignerCertificates(output) {
  return [...output.matchAll(/^(.*?)\bcertificate SHA-256 digest: ([\da-f]{64})\s*$/gim)]
    .filter((match) => !/^\s*Source Stamp\b/i.test(match[1]))
    .map((match) => match[2].toLowerCase());
}

export function verifyGoogleSignedApk({ badging, certificates, plan, certificateSha256 }) {
  const { architectures } = validateAndroidBadging(badging, plan);
  const signers = apkSignerCertificates(certificates);
  assert(signers.length > 0, `apksigner reported no signing certificate:\n${certificates}`);
  assert(
    signers.includes(certificateSha256),
    `Play signing certificate ${certificateSha256} is not among the APK signers ${signers.join(", ")}`,
  );
  return { certificateSha256, signers: [...new Set(signers)], architectures };
}

async function main() {
  const { values } = parseArgs({
    options: Object.fromEntries(
      ["version", "version-code", "build-tools", "output-dir"].map((key) => [
        key,
        { type: "string" },
      ]),
    ),
  });
  for (const key of ["version", "version-code", "build-tools", "output-dir"])
    assert(values[key], `Missing --${key}`);
  assert.match(values["version-code"], /^[1-9]\d*$/, "Invalid Play versionCode");
  const account = JSON.parse(process.env.PLAY_SERVICE_ACCOUNT_JSON ?? "{}");
  const plan = {
    version: values.version,
    versionCode: Number(values["version-code"]),
    packageId: "dev.ait.mobile",
  };
  const name = `Ait-${plan.version}-android.apk`;
  await mkdir(values["output-dir"], { recursive: true });
  const apk = path.join(values["output-dir"], name);
  const certificateSha256 = await downloadUniversalApk({
    packageName: plan.packageId,
    versionCode: plan.versionCode,
    accessToken: await requestAccessToken(account, globalThis.fetch),
    output: apk,
    fetch: globalThis.fetch,
  });
  try {
    const tool = (tool, args) =>
      execFileSync(path.join(values["build-tools"], tool), args, { encoding: "utf8" });
    const verified = verifyGoogleSignedApk({
      badging: tool("aapt", ["dump", "badging", apk]),
      certificates: tool("apksigner", ["verify", "--print-certs", apk]),
      plan,
      certificateSha256,
    });
    console.log(JSON.stringify({ name, versionCode: plan.versionCode, ...verified }, null, 2));
  } catch (error) {
    await rm(apk, { force: true });
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(`::error::${error.message}`);
    process.exitCode = 1;
  });
}
