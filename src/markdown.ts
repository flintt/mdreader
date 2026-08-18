import DOMPurify from "dompurify";
import { parseMarkdown } from "./markdown-parser";

interface RenderResponse {
  id: number;
  html?: string;
  error?: string;
}

let requestId = 0;
let worker: Worker | null = null;
const pending = new Map<number, { resolve: (html: string) => void; reject: (error: Error) => void }>();

function getWorker(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./renderer.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (event: MessageEvent<RenderResponse>) => {
    const request = pending.get(event.data.id);
    if (!request) return;
    pending.delete(event.data.id);
    if (event.data.error) request.reject(new Error(event.data.error));
    else request.resolve(event.data.html ?? "");
  };
  worker.onerror = () => {
    for (const request of pending.values()) request.reject(new Error("渲染线程异常"));
    pending.clear();
    worker?.terminate();
    worker = null;
  };
  return worker;
}

export async function renderMarkdown(source: string): Promise<string> {
  let parsed: string;
  try {
    const id = ++requestId;
    parsed = await new Promise<string>((resolve, reject) => {
      pending.set(id, { resolve, reject });
      getWorker().postMessage({ id, source });
    });
  } catch {
    parsed = parseMarkdown(source);
  }
  return DOMPurify.sanitize(parsed, {
    USE_PROFILES: { html: true },
    ADD_ATTR: ["target"],
  });
}

export function readingTime(source: string): number {
  const latinWords = source.match(/[A-Za-z0-9_]+/g)?.length ?? 0;
  const cjkCharacters = source.match(/[\u3400-\u9fff\uf900-\ufaff]/g)?.length ?? 0;
  return Math.max(1, Math.ceil(latinWords / 220 + cjkCharacters / 400));
}
