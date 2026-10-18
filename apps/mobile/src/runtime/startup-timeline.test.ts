import { describe, expect, it, vi } from "vitest";
import { createStartupTimeline, formatStartupMarks } from "./startup-timeline";

describe("startup timeline", () => {
  it("records each phase once, in time order, relative to its creation", () => {
    let now = 1_000;
    const timeline = createStartupTimeline({ now: () => now, report: vi.fn() });
    now = 1_120;
    timeline.mark("registry_loaded");
    now = 1_900;
    timeline.mark("host_online");
    now = 2_500;
    timeline.mark("registry_loaded");
    expect(timeline.marks()).toEqual([
      { phase: "registry_loaded", ms: 120 },
      { phase: "host_online", ms: 900 },
    ]);
  });

  it("reports once when the first directory is ready or the deadline passes", () => {
    let now = 0;
    let deadline: (() => void) | undefined;
    const report = vi.fn();
    const timeline = createStartupTimeline({
      now: () => now,
      report,
      setTimer: (callback) => {
        deadline = callback;
      },
    });
    now = 300;
    timeline.mark("host_online");
    expect(report).not.toHaveBeenCalled();
    now = 450;
    timeline.mark("directory_ready");
    deadline?.();
    expect(report).toHaveBeenCalledOnce();
    expect(formatStartupMarks(report.mock.calls[0]![0])).toBe(
      "host_online=300ms directory_ready=450ms",
    );
  });

  it("formats an empty timeline", () => {
    expect(formatStartupMarks([])).toBe("none");
  });
});
