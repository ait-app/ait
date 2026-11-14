import { describeCompactTimeAgo } from "@/utils/time";
import { useRelativeTimeLabel } from "./use-time-ago";

/**
 * A compact relative timestamp that keeps itself current.
 *
 * Call this from the smallest component that renders the text. The state lives there, so a tick
 * re-renders one `<Text>` and nothing above it — the row, the list, and the sidebar are never
 * involved.
 *
 * Two things keep the cost near zero:
 *
 * - **Ticking is not re-rendering.** A tick recomputes the label and only sets state if the
 *   string actually changed. A row reading "3d" is woken hourly and re-renders roughly once a
 *   day, when it becomes "4d".
 * - **The tier follows the label.** As a timestamp ages the label changes more slowly, so the
 *   subscription moves to a slower tier; past a week it unsubscribes for good, because a date
 *   never changes again.
 */
export function useCompactTimeAgo(date: Date | null): string {
  return useRelativeTimeLabel(date, describeCompactTimeAgo);
}
