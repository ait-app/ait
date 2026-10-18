/**
 * First-occurrence timings of the cold-start connection path, in milliseconds since this
 * module loaded (HostRuntime imports it while the JS bundle starts). Phases run in
 * parallel, so durations must be read from the marks rather than summed.
 */
export type StartupPhase =
  | "registry_loaded"
  | "boot_done"
  | "account_restored"
  | "node_registered"
  | "hosts_listed"
  | "relay_tentative"
  | "relay_available"
  | "host_connecting"
  | "host_online"
  | "directory_ready";

export interface StartupMark {
  phase: StartupPhase;
  ms: number;
}

interface StartupTimeline {
  mark(phase: StartupPhase): void;
  marks(): StartupMark[];
}

/** The summary is logged once the first host has loaded its directory, or after this long. */
const REPORT_DEADLINE_MS = 30_000;

export function createStartupTimeline(input: {
  now: () => number;
  report: (marks: StartupMark[]) => void;
  setTimer?: (callback: () => void, ms: number) => unknown;
}): StartupTimeline {
  const origin = input.now();
  const recorded = new Map<StartupPhase, number>();
  let reported = false;
  const report = () => {
    if (reported) return;
    reported = true;
    input.report(marks());
  };
  const marks = (): StartupMark[] =>
    [...recorded].map(([phase, ms]) => ({ phase, ms })).sort((left, right) => left.ms - right.ms);
  input.setTimer?.(report, REPORT_DEADLINE_MS);
  return {
    mark(phase) {
      if (recorded.has(phase)) return;
      recorded.set(phase, Math.round(input.now() - origin));
      if (phase === "directory_ready") report();
    },
    marks,
  };
}

export function formatStartupMarks(marks: readonly StartupMark[]): string {
  return marks.length === 0 ? "none" : marks.map(({ phase, ms }) => `${phase}=${ms}ms`).join(" ");
}

const timeline = createStartupTimeline({
  now: () => performance.now(),
  report: (marks) => console.info(`[Startup] ${formatStartupMarks(marks)}`),
  setTimer: (callback, ms) => {
    const timer = setTimeout(callback, ms);
    (timer as { unref?: () => void }).unref?.();
    return timer;
  },
});

export const markStartupPhase = timeline.mark;
export const getStartupMarks = timeline.marks;
