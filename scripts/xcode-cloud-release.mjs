import assert from "node:assert/strict";
import { createSign } from "node:crypto";
import { appendFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";

const ORIGIN = "https://api.appstoreconnect.apple.com";
const APP_ID = "6817043477";
const WORKFLOW_NAME = "Default";
const STABLE_TAG = /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/;
// Xcode Cloud can take a few minutes to list a freshly pushed tag.
const TAG_LOOKUP_ATTEMPTS = 21;
const TAG_LOOKUP_INTERVAL_MS = 30_000;
const READ_ATTEMPTS = 4;
const READ_RETRY_MS = 5_000;

export function createAppStoreConnectToken({ issuerId, keyId, privateKey, now = Date.now() }) {
  const iat = Math.floor(now / 1000);
  const encode = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
  const header = encode({ alg: "ES256", kid: keyId, typ: "JWT" });
  const payload = encode({ iss: issuerId, iat, exp: iat + 600, aud: "appstoreconnect-v1" });
  const signature = createSign("SHA256")
    .update(`${header}.${payload}`)
    .sign({ key: privateKey, dsaEncoding: "ieee-p1363" })
    .toString("base64url");
  return `${header}.${payload}.${signature}`;
}

function appStoreConnect({ fetch, token, sleep }) {
  async function request(target, init = {}) {
    const url = new URL(target, ORIGIN);
    // Pagination links come from the response; never send the token to another host.
    assert.equal(url.origin, ORIGIN, "App Store Connect returned a link to another host");
    // Only reads retry: repeating a POST could start a second build.
    const attempts = init.method === undefined ? READ_ATTEMPTS : 1;
    for (let attempt = 1; ; attempt += 1) {
      const canRetry = attempt < attempts;
      let response;
      try {
        response = await fetch(url.href, {
          ...init,
          headers: { Authorization: `Bearer ${token()}`, "Content-Type": "application/json" },
        });
      } catch (error) {
        if (!canRetry) throw error;
      }
      if (response) {
        const body = await response.json().catch(() => null);
        if (response.ok) return body;
        if (!canRetry || (response.status !== 429 && response.status < 500)) {
          const detail = body?.errors?.map((error) => error.detail ?? error.title).join("; ");
          throw new Error(
            `App Store Connect ${url.pathname} returned ${response.status}: ${detail ?? "no detail"}`,
          );
        }
      }
      await sleep(READ_RETRY_MS);
    }
  }
  async function list(target) {
    const items = [];
    for (let next = target; next; ) {
      const page = await request(next);
      items.push(...page.data);
      next = page.links?.next;
    }
    return items;
  }
  return { request, list };
}

export async function startTestFlightBuild({
  tag,
  fetch,
  token,
  sleep = delay,
  attempts = TAG_LOOKUP_ATTEMPTS,
}) {
  assert.match(tag, STABLE_TAG, "Expected a stable release tag vX.Y.Z");
  const api = appStoreConnect({ fetch, token, sleep });
  const product = await api.request(`/v1/apps/${APP_ID}/ciProduct`);
  const workflows = await api.list(`/v1/ciProducts/${product.data.id}/workflows?limit=200`);
  const matches = workflows.filter((item) => item.attributes.name === WORKFLOW_NAME);
  assert.equal(matches.length, 1, `Expected one Xcode Cloud workflow named ${WORKFLOW_NAME}`);
  const [workflow] = matches;
  assert.equal(workflow.attributes.isEnabled, true, `${WORKFLOW_NAME} workflow is disabled`);
  const repository = await api.request(`/v1/ciWorkflows/${workflow.id}/repository`);
  const references = `/v1/scmRepositories/${repository.data.id}/gitReferences?limit=200`;
  let reference;
  for (let attempt = 1; attempt <= attempts && !reference; attempt += 1) {
    reference = (await api.list(references)).find(
      (item) =>
        item.attributes.canonicalName === `refs/tags/${tag}` && item.attributes.isDeleted !== true,
    );
    if (!reference && attempt < attempts) await sleep(TAG_LOOKUP_INTERVAL_MS);
  }
  assert(reference, `Xcode Cloud does not list tag ${tag}`);
  const { data: run } = await api.request("/v1/ciBuildRuns", {
    method: "POST",
    body: JSON.stringify({
      data: {
        type: "ciBuildRuns",
        relationships: {
          workflow: { data: { type: "ciWorkflows", id: workflow.id } },
          sourceBranchOrTag: { data: { type: "scmGitReferences", id: reference.id } },
        },
      },
    }),
  });
  return { id: run.id, number: run.attributes?.number, workflow: WORKFLOW_NAME, tag };
}

async function main() {
  const [tag] = process.argv.slice(2);
  const issuerId = process.env.APP_STORE_CONNECT_ISSUER_ID;
  const keyId = process.env.APP_STORE_CONNECT_KEY_ID;
  const privateKey = process.env.APP_STORE_CONNECT_PRIVATE_KEY;
  assert(
    issuerId && keyId && privateKey,
    "APP_STORE_CONNECT_ISSUER_ID, _KEY_ID and _PRIVATE_KEY are required",
  );
  const build = await startTestFlightBuild({
    tag,
    fetch: globalThis.fetch,
    token: () => createAppStoreConnectToken({ issuerId, keyId, privateKey }),
  });
  const summary = [
    "## Ait iOS TestFlight",
    `- Tag: ${build.tag}`,
    `- Xcode Cloud workflow: ${build.workflow}`,
    `- Build run: ${build.number ?? build.id}`,
    `- Progress: https://appstoreconnect.apple.com/apps/${APP_ID}`,
  ].join("\n");
  console.log(summary);
  if (process.env.GITHUB_STEP_SUMMARY)
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${summary}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(`::error::${error.message}`);
    process.exitCode = 1;
  });
}
