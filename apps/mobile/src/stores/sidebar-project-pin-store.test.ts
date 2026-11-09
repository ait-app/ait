import { beforeEach, expect, it, vi } from "vitest";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { useSidebarProjectPinStore } from "./sidebar-project-pin-store";

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
  await useSidebarProjectPinStore.persist.rehydrate();
  useSidebarProjectPinStore.setState({ pinnedProjectKeys: [] });
});

it("puts new pins first and removes pins independently", () => {
  const { toggleProjectPin } = useSidebarProjectPinStore.getState();
  toggleProjectPin(" a ");
  toggleProjectPin("b");
  toggleProjectPin(" ");
  expect(useSidebarProjectPinStore.getState().pinnedProjectKeys).toEqual(["b", "a"]);
  toggleProjectPin("a");
  expect(useSidebarProjectPinStore.getState().pinnedProjectKeys).toEqual(["b"]);
});

it("restores pin order from persisted storage", async () => {
  await AsyncStorage.setItem(
    "sidebar-project-pins",
    JSON.stringify({ state: { pinnedProjectKeys: ["b", "a"] }, version: 0 }),
  );
  await useSidebarProjectPinStore.persist.rehydrate();
  expect(useSidebarProjectPinStore.getState().pinnedProjectKeys).toEqual(["b", "a"]);
});
