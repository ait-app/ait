// Called only while holding the nightly-dev-control job concurrency lock.
async function listRuns({ github, context }) {
  const runs = await github.paginate(github.rest.actions.listWorkflowRuns, {
    ...context.repo,
    workflow_id: "nightly.yml",
    branch: "dev",
    per_page: 100,
  });
  return runs
    .filter((run) => run.head_branch === "dev" && ["push", "workflow_dispatch"].includes(run.event))
    .sort((a, b) => b.run_number - a.run_number);
}

async function publishedNumber({ github, context }, runs) {
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
  if (marker) return Number(marker[1]);

  // Migrate the pre-window nightly using its recorded source commit.
  const commit = release.body?.match(/Development build from ([0-9a-f]{40})\b/)?.[1];
  const source = runs.find((run) => run.head_sha === commit);
  if (source) return source.run_number;
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
  const published = await publishedNumber(api, runs);
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
  return Boolean(current && current.run_number >= (await publishedNumber(api, runs)));
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
