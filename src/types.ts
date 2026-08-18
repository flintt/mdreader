export interface MarkdownFile {
  path: string;
  name: string;
  relativePath: string;
  depth: number;
}

export interface FileDocument {
  path: string;
  name: string;
  parent: string;
  content: string;
  modifiedMs: number;
  size: number;
}

export interface RecentFile {
  path: string;
  name: string;
  openedAt: number;
}

export type ThemePreference = "system" | "light" | "dark";

export interface ReaderSettings {
  theme: ThemePreference;
  fontSize: number;
  contentWidth: number;
  sidebarOpen: boolean;
}
