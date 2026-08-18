import { parseMarkdown } from "./markdown-parser";

self.onmessage = (event: MessageEvent<{ id: number; source: string }>) => {
  const { id, source } = event.data;
  try {
    self.postMessage({ id, html: parseMarkdown(source) });
  } catch (error) {
    self.postMessage({ id, error: error instanceof Error ? error.message : String(error) });
  }
};
