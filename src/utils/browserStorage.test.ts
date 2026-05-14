import { afterEach, describe, expect, it, vi } from "vitest";
import {
  getLocalStorageItem,
  removeLocalStorageItem,
  setLocalStorageItem,
} from "./browserStorage";

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");

function installWindow(value: unknown): void {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value,
  });
}

afterEach(() => {
  if (originalWindow) {
    Object.defineProperty(globalThis, "window", originalWindow);
  } else {
    delete (globalThis as { window?: unknown }).window;
  }
});

describe("browserStorage", () => {
  it("treats null localStorage as unavailable", () => {
    installWindow({ localStorage: null });

    expect(getLocalStorageItem("missing")).toBeNull();
    expect(() => setLocalStorageItem("key", "value")).not.toThrow();
    expect(() => removeLocalStorageItem("key")).not.toThrow();
  });

  it("treats throwing localStorage access as unavailable", () => {
    installWindow(
      Object.defineProperty({}, "localStorage", {
        get: () => {
          throw new Error("blocked");
        },
      }),
    );

    expect(getLocalStorageItem("missing")).toBeNull();
    expect(() => setLocalStorageItem("key", "value")).not.toThrow();
    expect(() => removeLocalStorageItem("key")).not.toThrow();
  });

  it("delegates to available localStorage", () => {
    const getItem = vi.fn(() => "stored");
    const setItem = vi.fn();
    const removeItem = vi.fn();
    installWindow({ localStorage: { getItem, setItem, removeItem } });

    expect(getLocalStorageItem("key")).toBe("stored");
    setLocalStorageItem("key", "value");
    removeLocalStorageItem("key");

    expect(getItem).toHaveBeenCalledWith("key");
    expect(setItem).toHaveBeenCalledWith("key", "value");
    expect(removeItem).toHaveBeenCalledWith("key");
  });
});
