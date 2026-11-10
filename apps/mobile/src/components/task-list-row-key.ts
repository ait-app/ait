import type { TodoEntry } from "@/types/stream";

export function taskListRowKey(item: TodoEntry, index: number): string {
  return item.id ?? `${item.text}-${index}`;
}
