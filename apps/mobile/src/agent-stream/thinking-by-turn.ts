import type { StreamItem, ThoughtItem } from "@/types/stream";

interface ThoughtGroup {
  index: number;
  sources: ThoughtItem[];
}

/** One presentation instance belongs to one agent's chronological timeline. */
export function createTurnThinkingProjection() {
  const rows = new WeakMap<ThoughtItem, { sources: ThoughtItem[]; row: ThoughtItem }>();
  let sourceTail: StreamItem[] | undefined;
  let sourceHead: StreamItem[] | undefined;
  let history: StreamItem[] = [];
  let historyGroups = new Map<string, ThoughtGroup>();
  let projection: { tail: StreamItem[]; head: StreamItem[] } | undefined;

  function thoughtRow(sources: ThoughtItem[]): ThoughtItem {
    const first = sources[0]!;
    const cached = rows.get(first);
    if (
      cached?.sources.length === sources.length &&
      cached.sources.every((source, index) => source === sources[index])
    ) {
      return cached.row;
    }
    const row: ThoughtItem = {
      ...first,
      id: first.turnId ? `thinking:turn:${encodeURIComponent(first.turnId)}` : first.id,
      text: sources
        .map((source) => source.text)
        .filter(Boolean)
        .join("\n\n"),
      status: sources[sources.length - 1]!.status,
      timestamp: first.turnId ? first.timestamp : sources[sources.length - 1]!.timestamp,
    };
    rows.set(first, { sources, row });
    return row;
  }

  function appendLegacy(items: StreamItem[], item: StreamItem, preceding: StreamItem | undefined) {
    const previous = items.at(-1);
    if (
      item.kind === "thought" &&
      previous?.kind === "thought" &&
      !previous.turnId &&
      preceding?.kind === "thought" &&
      !preceding.turnId
    ) {
      items[items.length - 1] = thoughtRow([previous, item]);
    } else {
      items.push(item);
    }
  }

  return (input: { tail: StreamItem[]; head: StreamItem[] }) => {
    if (projection && sourceTail === input.tail && sourceHead === input.head) return projection;
    if (sourceTail !== input.tail) {
      const grouped: StreamItem[] = [];
      historyGroups = new Map();
      for (const [index, item] of input.tail.entries()) {
        if (item.kind !== "thought" || !item.turnId) {
          appendLegacy(grouped, item, input.tail[index - 1]);
          continue;
        }
        const group = historyGroups.get(item.turnId);
        if (group) {
          group.sources.push(item);
        } else {
          historyGroups.set(item.turnId, { index: grouped.length, sources: [item] });
          grouped.push(item);
        }
      }
      for (const group of historyGroups.values()) grouped[group.index] = thoughtRow(group.sources);
      history =
        historyGroups.size === 0 && grouped.length === input.tail.length ? input.tail : grouped;
      sourceTail = input.tail;
    }

    let tail = history;
    const head: StreamItem[] = [];
    const liveGroups = new Map<string, ThoughtGroup>();
    for (const [index, item] of input.head.entries()) {
      if (item.kind !== "thought" || !item.turnId) {
        appendLegacy(head, item, input.head[index - 1]);
        continue;
      }
      const group = liveGroups.get(item.turnId);
      if (group) {
        group.sources.push(item);
      } else {
        liveGroups.set(item.turnId, { index: head.length, sources: [item] });
        if (!historyGroups.has(item.turnId)) head.push(item);
      }
    }
    for (const [turnId, group] of liveGroups) {
      const previous = historyGroups.get(turnId);
      if (previous) {
        if (tail === history) tail = [...history];
        tail[previous.index] = thoughtRow([...previous.sources, ...group.sources]);
      } else {
        head[group.index] = thoughtRow(group.sources);
      }
    }
    const previous = tail.at(-1);
    const next = head[0];
    const previousSource = input.tail.at(-1);
    const nextSource = input.head[0];
    if (
      previous?.kind === "thought" &&
      !previous.turnId &&
      next?.kind === "thought" &&
      !next.turnId &&
      previousSource?.kind === "thought" &&
      !previousSource.turnId &&
      nextSource?.kind === "thought" &&
      !nextSource.turnId
    ) {
      tail = [...tail.slice(0, -1), thoughtRow([previous, next])];
      head.shift();
    }
    sourceHead = input.head;
    projection = {
      tail,
      head: liveGroups.size === 0 && head.length === input.head.length ? input.head : head,
    };
    return projection;
  };
}
