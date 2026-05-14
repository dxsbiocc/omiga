export function getLocalStorage(): Storage | null {
  try {
    if (typeof window === "undefined") return null;
    const storage = window.localStorage;
    if (!storage || typeof storage.getItem !== "function") return null;
    return storage;
  } catch {
    return null;
  }
}

export function getLocalStorageItem(key: string): string | null {
  try {
    return getLocalStorage()?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

export function setLocalStorageItem(key: string, value: string): void {
  try {
    getLocalStorage()?.setItem(key, value);
  } catch {
    /* localStorage can be unavailable/null in restricted webviews */
  }
}

export function removeLocalStorageItem(key: string): void {
  try {
    getLocalStorage()?.removeItem(key);
  } catch {
    /* localStorage can be unavailable/null in restricted webviews */
  }
}

export const safeLocalStorage = {
  getItem: getLocalStorageItem,
  setItem: setLocalStorageItem,
  removeItem: removeLocalStorageItem,
};
