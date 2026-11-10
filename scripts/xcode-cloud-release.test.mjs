import assert from "node:assert/strict";
import { createVerify, generateKeyPairSync } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";
import yaml from "yaml";
import { createAppStoreConnectToken, startTestFlightBuild } from "./xcode-cloud-release.mjs";

const workflowFile = yaml.parse(
  await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
);
const job = workflowFile.jobs["ios-testflight"];
const origin = "https://api.appstoreconnect.apple.com";
const defaultWorkflow = { id: "wf", attributes: { name: "Default", isEnabled: true } };
const tagReference = {
  id: "ref-tag",
  attributes: { canonicalName: "refs/tags/v1.2.3", isDeleted: false },
};

function fakeApi(overrides = {}) {
  const calls = [];
  const routes = {
    "/v1/apps/6817043477/ciProduct": { data: { id: "product" } },
    "/v1/ciProducts/product/workflows?limit=200": {
      data: [
        { id: "other", attributes: { name: "Release iOS to App Store", isEnabled: true } },
        defaultWorkflow,
      ],
    },
    "/v1/ciWorkflows/wf/repository": { data: { id: "repo" } },
    "/v1/scmRepositories/repo/gitReferences?limit=200": () => ({ data: [tagReference] }),
    "/v1/ciBuildRuns": { data: { id: "run", attributes: { number: 42 } } },
    ...overrides,
  };
  const fetch = async (url, init) => {
    const target = url.slice(origin.length);
    calls.push({ target, url, init });
    const route = routes[target];
    assert(route, `Unexpected request ${url}`);
    const value = typeof route === "function" ? route() : route;
    return { ok: value.status === undefined, status: value.status ?? 200, json: async () => value };
  };
  return { calls, fetch };
}

const start = (fetch, extra = {}) =>
  startTestFlightBuild({
    tag: "v1.2.3",
    fetch,
    token: () => "secret-token",
    sleep: async () => {},
    attempts: 3,
    ...extra,
  });

test("token is a short-lived ES256 JWT for App Store Connect", () => {
  const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const now = 1_800_000_000_000;
  const token = createAppStoreConnectToken({ issuerId: "issuer", keyId: "KEY", privateKey, now });
  const [header, payload, signature] = token.split(".");
  const decode = (part) => JSON.parse(Buffer.from(part, "base64url").toString());
  assert.deepEqual(decode(header), { alg: "ES256", kid: "KEY", typ: "JWT" });
  const claims = decode(payload);
  assert.equal(claims.iss, "issuer");
  assert.equal(claims.aud, "appstoreconnect-v1");
  assert(claims.exp > claims.iat && claims.exp - claims.iat <= 20 * 60);
  const verified = createVerify("SHA256")
    .update(`${header}.${payload}`)
    .verify({ key: publicKey, dsaEncoding: "ieee-p1363" }, Buffer.from(signature, "base64url"));
  assert(verified);
});

test("starts the Default workflow on the exact tag, once", async () => {
  const { calls, fetch } = fakeApi();
  const build = await start(fetch);
  assert.deepEqual(build, { id: "run", number: 42, workflow: "Default", tag: "v1.2.3" });
  const posts = calls.filter((call) => call.init.method === "POST");
  assert.equal(posts.length, 1);
  assert.equal(posts[0].target, "/v1/ciBuildRuns");
  assert.equal(posts[0].init.headers.Authorization, "Bearer secret-token");
  assert.deepEqual(JSON.parse(posts[0].init.body), {
    data: {
      type: "ciBuildRuns",
      relationships: {
        workflow: { data: { type: "ciWorkflows", id: "wf" } },
        sourceBranchOrTag: { data: { type: "scmGitReferences", id: "ref-tag" } },
      },
    },
  });
});

test("waits for a new tag, ignores lookalike refs and follows pagination", async () => {
  const pageTwo = `${origin}/v1/scmRepositories/repo/gitReferences?cursor=2`;
  const workflowPage = `${origin}/v1/ciProducts/product/workflows?cursor=2`;
  const lookalike = (id, canonicalName) => ({
    id,
    attributes: { canonicalName, isDeleted: false },
  });
  let lookups = 0;
  const { calls, fetch } = fakeApi({
    "/v1/ciProducts/product/workflows?limit=200": { data: [], links: { next: workflowPage } },
    "/v1/ciProducts/product/workflows?cursor=2": { data: [defaultWorkflow] },
    "/v1/scmRepositories/repo/gitReferences?limit=200": () => {
      lookups += 1;
      return lookups === 1
        ? { data: [lookalike("old", "refs/heads/v1.2.3")] }
        : { data: [lookalike("near", "refs/tags/xv1.2.3")], links: { next: pageTwo } };
    },
    "/v1/scmRepositories/repo/gitReferences?cursor=2": { data: [tagReference] },
  });
  const sleeps = [];
  assert.equal((await start(fetch, { sleep: async (ms) => sleeps.push(ms) })).id, "run");
  assert.equal(lookups, 2);
  assert.deepEqual(sleeps, [30_000]);
  const body = JSON.parse(calls.find((call) => call.init.method === "POST").init.body);
  assert.equal(body.data.relationships.sourceBranchOrTag.data.id, "ref-tag");
});

test("transient read errors retry, but a failed start is never repeated", async () => {
  const unavailable = { status: 503, errors: [{ detail: "Try later" }] };
  let lookups = 0;
  const flaky = fakeApi({
    "/v1/scmRepositories/repo/gitReferences?limit=200": () => {
      lookups += 1;
      if (lookups === 1) throw new TypeError("fetch failed");
      return lookups === 2 ? { status: 429, errors: [] } : { data: [tagReference] };
    },
  });
  assert.equal((await start(flaky.fetch)).id, "run");
  assert.equal(lookups, 3);
  const failedStart = fakeApi({ "/v1/ciBuildRuns": unavailable });
  await assert.rejects(start(failedStart.fetch), /returned 503: Try later/);
  assert.equal(failedStart.calls.filter((call) => call.init.method === "POST").length, 1);
  const forbidden = fakeApi({
    "/v1/ciWorkflows/wf/repository": { status: 403, errors: [{ detail: "Denied" }] },
  });
  await assert.rejects(start(forbidden.fetch), /returned 403: Denied/);
  assert.equal(
    forbidden.calls.filter((call) => call.target === "/v1/ciWorkflows/wf/repository").length,
    1,
  );
});

test("never starts a build for an unsafe request or unresolved target", async () => {
  const noPost = async (api, expected, extra) => {
    await assert.rejects(start(api.fetch, extra), expected);
    assert(!api.calls.some((call) => call.init.method === "POST"));
  };
  for (const tag of ["v1.2.3-beta.1", "main", "v1.2.3\nmain", undefined]) {
    const api = fakeApi();
    await assert.rejects(start(api.fetch, { tag }), /stable release tag|string/);
    assert.equal(api.calls.length, 0);
  }
  await noPost(
    fakeApi({ "/v1/ciProducts/product/workflows?limit=200": { data: [] } }),
    /one Xcode Cloud workflow/,
  );
  await noPost(
    fakeApi({
      "/v1/ciProducts/product/workflows?limit=200": {
        data: [{ id: "wf", attributes: { name: "Default", isEnabled: false } }],
      },
    }),
    /disabled/,
  );
  await noPost(
    fakeApi({ "/v1/scmRepositories/repo/gitReferences?limit=200": () => ({ data: [] }) }),
    /does not list tag v1\.2\.3/,
  );
  await noPost(
    fakeApi({
      "/v1/scmRepositories/repo/gitReferences?limit=200": () => ({
        data: [{ id: "x", attributes: { canonicalName: "refs/tags/v1.2.3", isDeleted: true } }],
      }),
    }),
    /does not list tag/,
  );
});

test("API errors explain themselves and pagination cannot leak the token", async () => {
  const { fetch } = fakeApi({
    "/v1/apps/6817043477/ciProduct": { status: 403, errors: [{ detail: "Role not allowed" }] },
  });
  await assert.rejects(start(fetch), (error) => {
    assert.match(error.message, /returned 403: Role not allowed/);
    assert.doesNotMatch(error.message, /secret-token/);
    return true;
  });
  const foreign = fakeApi({
    "/v1/scmRepositories/repo/gitReferences?limit=200": () => ({
      data: [],
      links: { next: "https://example.com/steal" },
    }),
  });
  await assert.rejects(start(foreign.fetch), /another host/);
  assert(!foreign.calls.some((call) => call.url.includes("example.com")));
});

test("release workflow starts stable iOS builds without blocking the GitHub Release", () => {
  assert.equal(
    job.if,
    "${{ !contains(inputs.tag || github.ref_name, '-') && !inputs.source_commit }}",
  );
  assert.equal(job["continue-on-error"], true);
  assert.equal(job.permissions, undefined);
  assert(!workflowFile.jobs.release.needs.includes("ios-testflight"));
  const position = (name) => job.steps.findIndex((step) => step.name === name);
  const source = job.steps[position("Check out release source")];
  const tooling = job.steps[position("Check out release tooling")];
  assert.equal(source.with.ref, "${{ env.SOURCE_REF }}");
  assert.equal(tooling.with.ref, "${{ github.workflow_sha }}");
  for (const checkout of [source, tooling])
    assert.equal(checkout.with["persist-credentials"], false);
  const start = job.steps.find((step) => step.name === "Start Xcode Cloud TestFlight build");
  assert(position("Verify release version") >= 0);
  assert(position("Verify release version") < job.steps.indexOf(start));
  assert.deepEqual(Object.keys(start.env).sort(), [
    "APP_STORE_CONNECT_ISSUER_ID",
    "APP_STORE_CONNECT_KEY_ID",
    "APP_STORE_CONNECT_PRIVATE_KEY",
  ]);
  assert.doesNotMatch(start.run, /\$\{\{/);
  assert.match(start.run, /xcode-cloud-release\.mjs "\$RELEASE_TAG"/);
});
