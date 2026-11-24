import { beforeEach, expect, it, vi } from "vitest";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { mergeMethodKey, useMergeMethodStore } from "./merge-method-store";

vi.mock("@react-native-async-storage/async-storage", () => {
  const entries = new Map<string, string>();
  return {
    default: {
      getItem: async (key: string) => entries.get(key) ?? null,
      setItem: async (key: string, value: string) => {
        entries.set(key, value);
      },
      removeItem: async (key: string) => {
        entries.delete(key);
      },
    },
  };
});

beforeEach(async () => {
  await useMergeMethodStore.persist.rehydrate();
  useMergeMethodStore.setState({ methods: {} });
});

it("reads back the method remembered for a host and project", () => {
  const key = mergeMethodKey("server-1", "project-1");
  useMergeMethodStore.getState().setMethod(key, "rebase");
  expect(useMergeMethodStore.getState().methods[key]).toBe("rebase");
});

it("keeps the methods of different projects apart", () => {
  const { setMethod } = useMergeMethodStore.getState();
  setMethod(mergeMethodKey("server-1", "project-1"), "rebase");
  setMethod(mergeMethodKey("server-1", "project-2"), "merge");
  setMethod(mergeMethodKey("server-2", "project-1"), "squash");
  const { methods } = useMergeMethodStore.getState();
  expect(methods[mergeMethodKey("server-1", "project-1")]).toBe("rebase");
  expect(methods[mergeMethodKey("server-1", "project-2")]).toBe("merge");
  expect(methods[mergeMethodKey("server-2", "project-1")]).toBe("squash");
  expect(mergeMethodKey("a:b", "c")).not.toBe(mergeMethodKey("a", "b:c"));
});

it("restores remembered methods from persisted storage", async () => {
  const key = mergeMethodKey("server-1", "project-1");
  await AsyncStorage.setItem(
    "git-merge-method",
    JSON.stringify({ state: { methods: { [key]: "merge" } }, version: 0 }),
  );
  await useMergeMethodStore.persist.rehydrate();
  expect(useMergeMethodStore.getState().methods[key]).toBe("merge");
});
