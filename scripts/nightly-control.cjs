// Called only while holding the nightly-main-control job concurrency lock.
async function listRuns({ github, context }) {
  const runs = await github.paginate(github.rest.actions.listWorkflowRuns, {
    ...context.repo,
    workflow_id: "ci.yml",
    branch: "main",
    per_page: 100,
  });
  return runs
    .filter(
      (run) => run.head_branch === "main" && ["push", "workflow_dispatch"].includes(run.event),
    )
    .sort((a, b) => b.run_number - a.run_number);
}

async function publishedNumber({ github, context }) {
  let release;
  try {
    ({ data: release } = await github.rest.repos.getReleaseByTag({
      ...context.repo,
      tag: "nightly",
    }));
  } catch (error) {
    if (error.status === 404) return 0;
    throw error;
  }
  const marker = release.body?.match(/<!-- ait-nightly-run: (\d+) -->/);
  if (marker && release.body.includes("<!-- ait-nightly-workflow: ci.yml -->")) {
    return Number(marker[1]);
  }
  // The former dev nightly workflow has its own run-number sequence.
  // Its watermark cannot be compared to CI; the first main publication replaces it.
  if (marker && !release.body.includes("<!-- ait-nightly-workflow:")) return 0;

  // Migrate the pre-window nightly using its recorded source commit.
  const commit = release.body?.match(/Development build from ([0-9a-f]{40})\b/)?.[1];
  if (commit && !release.body.includes("<!-- ait-nightly-workflow:")) return 0;
  throw new Error("Cannot determine the published nightly build number; refusing to overwrite it.");
}

async function cancelRuns({ github, context, core }, runs) {
  for (const run of runs) {
    if (run.id === context.runId || run.status === "completed") continue;
    try {
      await github.rest.actions.cancelWorkflowRun({ ...context.repo, run_id: run.id });
      core.notice(`Cancelled superseded nightly #${run.run_number}.`);
    } catch (error) {
      // The run may have finished since it was listed. Other errors must remain visible.
      if (error.status !== 409) throw error;
      core.notice(`Nightly #${run.run_number} is no longer cancellable.`);
    }
  }
}

async function admitBuild(api) {
  const runs = await listRuns(api);
  const published = await publishedNumber(api);
  const window = runs.slice(0, 2);
  await cancelRuns(
    api,
    runs.filter((run) => !window.includes(run) || run.run_number < published),
  );
  return window.some((run) => run.id === api.context.runId && run.run_number >= published);
}

async function canPublish(api) {
  const runs = await listRuns(api);
  const current = runs.slice(0, 2).find((run) => run.id === api.context.runId);
  return Boolean(current && current.run_number >= (await publishedNumber(api)));
}

async function cancelOlderBuilds(api) {
  const runs = await listRuns(api);
  const current = runs.find((run) => run.id === api.context.runId);
  if (!current) throw new Error("Current nightly run is missing from the workflow run list.");
  await cancelRuns(
    api,
    runs.filter((run) => run.run_number < current.run_number),
  );
}

module.exports = { admitBuild, canPublish, cancelOlderBuilds };
