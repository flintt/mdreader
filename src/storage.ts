import type { ReaderSettings, RecentFile } from "./types";

const SETTINGS_KEY = "mdreader.settings.v1";
const RECENTS_KEY = "mdreader.recents.v1";

export const defaultSettings: ReaderSettings = {
  theme: "system",
  fontSize: 17,
  contentWidth: 780,
  sidebarOpen: true,
};

export function loadSettings(): ReaderSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "{}");
    return { ...defaultSettings, ...saved };
  } catch {
    return { ...defaultSettings };
  }
}

export function saveSettings(settings: ReaderSettings): void {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
}

export function loadRecents(): RecentFile[] {
  try {
    return JSON.parse(localStorage.getItem(RECENTS_KEY) ?? "[]");
  } catch {
    return [];
  }
}

export function addRecent(file: RecentFile): RecentFile[] {
  const recents = loadRecents().filter((item) => item.path !== file.path);
  const next = [file, ...recents].slice(0, 8);
  localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  return next;
}

export function clearRecents(): void {
  localStorage.removeItem(RECENTS_KEY);
}
