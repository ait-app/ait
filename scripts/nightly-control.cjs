// Admission and tag handling for the manual .github/workflows/nightly.yml. Runs are serialized
// by the workflow's concurrency group, and each one publishes main's commit at trigger time, so
// the published nightly only moves forward.

const CI_WORKFLOW = "ci.yml";
const POLL_INTERVAL_MS = 30_000;
// One hour of polling; the prepare job's timeout leaves room for the API calls around it.
const POLL_ATTEMPTS = 120;

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

// The latest CI result for this commit's push to main: a conclusion, or "pending".
async function ciResult({ github, context }) {
  const { data } = await github.rest.actions.listWorkflowRuns({
    ...context.repo,
    workflow_id: CI_WORKFLOW,
    head_sha: context.sha,
    event: "push",
    per_page: 20,
  });
  const [run] = data.workflow_runs
    .filter((candidate) => candidate.head_branch === "main")
    .sort((a, b) => b.run_number - a.run_number);
  // A push's CI run can appear a moment after the push itself.
  if (!run || run.status !== "completed") return "pending";
  return run.conclusion;
}

// Whether this run should build and publish `context.sha`: CI must have passed on the same
// commit. A CI run that is still in progress is awaited.
async function admit({ github, context, core, wait = sleep, attempts = POLL_ATTEMPTS }) {
  for (let attempt = 1; ; attempt += 1) {
    const result = await ciResult({ github, context });
    if (result === "success") return true;
    if (result !== "pending" || attempt >= attempts) {
      core.warning(`CI on ${context.sha} is ${result}; the previous nightly stays published.`);
      return false;
    }
    await wait(POLL_INTERVAL_MS);
  }
}

// A repository or organization rule refused to change a protected ref.
function protectedRef(error) {
  return error.status === 422 && /rule violation|protected ref/i.test(error.message ?? "");
}

// Point the `nightly` tag at this build before the published release is touched, so a refused
// ref update can never leave the channel without a release. When a rule protects the tag, it
// stays where it is and the release still publishes this build; the notes and BUILD-INFO.json
// record the actual source commit.
async function moveTag({ github, context, core }) {
  const ref = { ...context.repo, ref: "tags/nightly" };
  let current;
  try {
    ({ data: current } = await github.rest.git.getRef(ref));
  } catch (error) {
    if (error.status !== 404) throw error;
    await github.rest.git.createRef({
      ...context.repo,
      ref: "refs/tags/nightly",
      sha: context.sha,
    });
    return;
  }
  if (current.object?.sha === context.sha) return;
  try {
    await github.rest.git.updateRef({ ...ref, sha: context.sha, force: true });
  } catch (error) {
    if (!protectedRef(error)) throw error;
    core.warning(
      `A ruleset protects the nightly tag, so it stays at ${current.object?.sha}. ` +
        `The release still publishes ${context.sha}. Allow GitHub Actions to bypass the rule ` +
        "to keep the tag on the published build.",
    );
  }
}

module.exports = { admit, moveTag };
