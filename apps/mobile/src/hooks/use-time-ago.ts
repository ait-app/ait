import { useEffect, useState } from "react";
import { subscribeToRelativeTimeTick, type TickResolution } from "@/utils/relative-time-ticker";
import { describeTimeAgo, type RelativeTimeLabel } from "@/utils/time";

/** A prose relative timestamp ("5m ago") that keeps itself current. */
export function useTimeAgo(date: Date | null): string {
  return useRelativeTimeLabel(date, describeTimeAgo);
}

function useRelativeTimeLabel(
  date: Date | null,
  describe: (date: Date) => RelativeTimeLabel,
): string {
  const [label, setLabel] = useState(() => (date ? describe(date).label : ""));

  // Keyed on the instant, not the Date object: the store parses a fresh Date on every payload, so
  // depending on identity would tear down and rebuild the subscription for an unchanged time.
  const time = date === null ? null : date.getTime();

  useEffect(() => {
    if (time === null) {
      setLabel("");
      return undefined;
    }

    const source = new Date(time);
    let current = describe(source);
    setLabel(current.label);

    let unsubscribe: (() => void) | null = null;

    const handleTick = () => {
      const next = describe(source);
      if (next.label !== current.label) {
        setLabel(next.label);
      }
      if (next.resolution !== current.resolution) {
        // Aged into a slower tier — or out of them entirely.
        current = next;
        unsubscribe?.();
        unsubscribe = null;
        attach();
        return;
      }
      current = next;
    };

    const attach = () => {
      if (current.resolution === "static") return;
      unsubscribe = subscribeToRelativeTimeTick(current.resolution as TickResolution, handleTick);
    };

    attach();
    return () => {
      unsubscribe?.();
    };
  }, [time, describe]);

  return label;
}
