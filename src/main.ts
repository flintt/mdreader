import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";
import { icon } from "./icons";
import { readingTime, renderMarkdown } from "./markdown";
import { addRecent, clearRecents, loadRecents, loadSettings, saveSettings } from "./storage";
import type { FileDocument, MarkdownFile, ReaderSettings, RecentFile, ThemePreference } from "./types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

type SidebarTab = "files" | "outline";

interface Heading {
  id: string;
  text: string;
  level: number;
}

interface CachedRender {
  modifiedMs: number;
  html: string;
}

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("App root not found");

const isTauri = window.__TAURI_INTERNALS__ !== undefined;
let settings: ReaderSettings = loadSettings();
let recents: RecentFile[] = loadRecents();
let documentState: FileDocument | null = null;
let directoryFiles: MarkdownFile[] = [];
let sidebarTab: SidebarTab = "files";
let filterText = "";
let lastRenderMs = 0;
let toastTimer = 0;
let loadSequence = 0;
let codeObserver: IntersectionObserver | null = null;
let mermaidObserver: IntersectionObserver | null = null;
let mermaidRenderQueue: Promise<void> = Promise.resolve();
let appliedTheme: "light" | "dark" | null = null;
const renderCache = new Map<string, CachedRender>();

app.innerHTML = `
  <main class="app-shell">
    <aside class="sidebar" aria-label="文档导航">
      <div class="sidebar-header">
        <div class="brand"><span class="brand-mark">${icon("book")}</span><span>MD Reader</span></div>
        <label class="search-box">${icon("search")}<input id="file-search" type="search" placeholder="筛选文件" autocomplete="off" /><kbd>Ctrl K</kbd></label>
      </div>
      <div class="sidebar-tabs">
        <button class="tab-button active" data-tab="files">文件</button>
        <button class="tab-button" data-tab="outline">大纲</button>
      </div>
      <div class="sidebar-content" id="sidebar-content"></div>
      <div class="sidebar-footer"><button id="open-folder">${icon("folder")}<span>打开文件夹</span><small>Ctrl ⇧ O</small></button></div>
    </aside>
    <section class="workspace">
      <header class="toolbar">
        <button class="icon-button" id="toggle-sidebar" title="显示/隐藏侧边栏 (Ctrl+B)">${icon("menu")}</button>
        <div class="toolbar-title"><strong id="document-title">MD Reader</strong><span id="document-path">快速、安静地阅读 Markdown</span></div>
        <button class="icon-button" id="open-file" title="打开 Markdown (Ctrl+O)">${icon("file")}</button>
        <span class="toolbar-separator"></span>
        <button class="icon-button" id="theme-toggle" title="切换主题">${icon("sun")}</button>
        <button class="icon-button" id="settings-toggle" title="阅读设置">${icon("settings")}</button>
      </header>
      <div class="reader-scroll" id="reader-scroll"><div class="empty-state" id="reader-view"></div></div>
      <footer class="statusbar"><span id="render-status"></span><span id="document-stats"></span></footer>
      <div class="popover" id="settings-popover" hidden>
        <h3>阅读设置</h3>
        <div class="theme-options">
          <button class="theme-button" data-theme-choice="system">${icon("monitor")}<span>跟随系统</span></button>
          <button class="theme-button" data-theme-choice="light">${icon("sun")}<span>浅色</span></button>
          <button class="theme-button" data-theme-choice="dark">${icon("moon")}<span>深色</span></button>
        </div>
        <label class="setting-row"><span>正文字号</span><output id="font-size-output"></output><input id="font-size" type="range" min="14" max="24" step="1" /></label>
        <label class="setting-row"><span>页面宽度</span><output id="content-width-output"></output><input id="content-width" type="range" min="580" max="1080" step="20" /></label>
      </div>
      <div class="drop-overlay" id="drop-overlay" hidden>松开即可打开 Markdown</div>
      <div class="toast" id="toast" role="status"></div>
    </section>
  </main>`;

const shell = required<HTMLElement>(".app-shell");
const readerView = required<HTMLElement>("#reader-view");
const readerScroll = required<HTMLElement>("#reader-scroll");
const sidebarContent = required<HTMLElement>("#sidebar-content");
const fileSearch = required<HTMLInputElement>("#file-search");
const titleElement = required<HTMLElement>("#document-title");
const pathElement = required<HTMLElement>("#document-path");
const statsElement = required<HTMLElement>("#document-stats");
const renderStatus = required<HTMLElement>("#render-status");
const settingsPopover = required<HTMLElement>("#settings-popover");
const dropOverlay = required<HTMLElement>("#drop-overlay");

function required<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing element: ${selector}`);
  return element;
}

function escapeHtml(value: string): string {
  const entities: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };
  return value.replace(/[&<>"]/g, (character) => entities[character] ?? character);
}

function applySettings(): void {
  const resolvedTheme = settings.theme === "system"
    ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
    : settings.theme;
  const themeChanged = appliedTheme !== null && appliedTheme !== resolvedTheme;
  appliedTheme = resolvedTheme;
  document.documentElement.dataset.theme = resolvedTheme;
  document.documentElement.style.setProperty("--reader-font-size", `${settings.fontSize}px`);
  document.documentElement.style.setProperty("--content-width", `${settings.contentWidth}px`);
  shell.classList.toggle("sidebar-hidden", !settings.sidebarOpen);
  required<HTMLInputElement>("#font-size").value = String(settings.fontSize);
  required<HTMLOutputElement>("#font-size-output").value = `${settings.fontSize}px`;
  required<HTMLInputElement>("#content-width").value = String(settings.contentWidth);
  required<HTMLOutputElement>("#content-width-output").value = `${settings.contentWidth}px`;
  document.querySelectorAll<HTMLElement>("[data-theme-choice]").forEach((button) => {
    button.classList.toggle("active", button.dataset.themeChoice === settings.theme);
  });
  required<HTMLElement>("#theme-toggle").innerHTML = icon(resolvedTheme === "dark" ? "moon" : "sun");
  if (themeChanged && documentState) rerenderMermaidDiagrams();
}

function updateSetting(patch: Partial<ReaderSettings>): void {
  settings = { ...settings, ...patch };
  saveSettings(settings);
  applySettings();
}

function showWelcome(): void {
  readerView.className = "empty-state";
  readerView.innerHTML = `
    <div class="welcome">
      <div class="welcome-logo">${icon("book")}</div>
      <h1>让文字先于工具出现</h1>
      <p>轻量、离线、无干扰的 Markdown 阅读器</p>
      <div class="welcome-actions">
        <button class="primary-button" data-action="open-file">打开 Markdown</button>
        <button class="secondary-button" data-action="open-folder">打开文件夹</button>
      </div>
      <div class="recent-list" id="recent-list"></div>
      <div class="drop-hint">也可以把 .md 文件直接拖到窗口中</div>
    </div>`;
  renderRecents();
}

function renderRecents(): void {
  const container = document.querySelector<HTMLElement>("#recent-list");
  if (!container) return;
  if (recents.length === 0) {
    container.innerHTML = "";
    return;
  }
  container.innerHTML = `
    <div class="section-label"><span>最近打开</span><button data-action="clear-recents">清除</button></div>
    ${recents.map((file) => `<button class="nav-item" data-recent-path="${escapeHtml(file.path)}" title="${escapeHtml(file.path)}">${icon("file")}<span>${escapeHtml(file.name)}</span></button>`).join("")}`;
}

async function pickFile(): Promise<void> {
  if (!isTauri) {
    showToast("浏览器预览模式不能读取本地文件，请运行 npm run tauri dev");
    return;
  }
  const selected = await open({
    multiple: false,
    directory: false,
    title: "打开 Markdown",
    filters: [{ name: "Markdown", extensions: ["md", "markdown", "mdown", "mkd", "mdx", "txt"] }],
  });
  if (typeof selected === "string") await loadFile(selected);
}

async function pickFolder(): Promise<void> {
  if (!isTauri) {
    showToast("浏览器预览模式不能读取本地文件，请运行 npm run tauri dev");
    return;
  }
  const selected = await open({ multiple: false, directory: true, title: "打开 Markdown 文件夹" });
  if (typeof selected !== "string") return;
  try {
    directoryFiles = await invoke<MarkdownFile[]>("scan_markdown_directory", { path: selected });
    sidebarTab = "files";
    if (!settings.sidebarOpen) updateSetting({ sidebarOpen: true });
    renderSidebar();
    if (directoryFiles.length > 0) await loadFile(directoryFiles[0].path);
    else showToast("这个文件夹里没有找到 Markdown 文件");
  } catch (error) {
    showToast(`无法打开文件夹：${String(error)}`);
  }
}

async function loadFile(path: string, anchor?: string): Promise<void> {
  if (!isTauri) return;
  const sequence = ++loadSequence;
  renderStatus.textContent = "读取中…";
  try {
    const file = await invoke<FileDocument>("read_markdown_file", { path });
    if (sequence !== loadSequence) return;
    const startedAt = performance.now();
    const cached = renderCache.get(file.path);
    const html = cached?.modifiedMs === file.modifiedMs ? cached.html : await renderMarkdown(file.content);
    if (sequence !== loadSequence) return;
    lastRenderMs = performance.now() - startedAt;
    cacheRender(file.path, { modifiedMs: file.modifiedMs, html });
    documentState = file;
    recents = addRecent({ path: file.path, name: file.name, openedAt: Date.now() });
    await showDocument(html, anchor);
    renderSidebar();
  } catch (error) {
    renderStatus.textContent = "";
    showToast(`无法打开文件：${String(error)}`);
  }
}

function cacheRender(path: string, value: CachedRender): void {
  renderCache.delete(path);
  renderCache.set(path, value);
  if (renderCache.size > 12) renderCache.delete(renderCache.keys().next().value ?? "");
}

async function showDocument(html: string, anchor?: string): Promise<void> {
  if (!documentState) return;
  readerView.className = "reader markdown-body";
  readerView.innerHTML = html;
  assignHeadingIds();
  scheduleMermaidRendering();
  scheduleCodeHighlighting();
  void hydrateLocalImages();
  titleElement.textContent = documentState.name.replace(/\.(md|markdown|mdown|mkd|mdx|txt)$/i, "");
  pathElement.textContent = documentState.path;
  const characterCount = documentState.content.length.toLocaleString("zh-CN");
  statsElement.textContent = `${characterCount} 字符 · 约 ${readingTime(documentState.content)} 分钟`;
  renderStatus.textContent = `渲染 ${lastRenderMs.toFixed(lastRenderMs < 10 ? 1 : 0)} ms`;
  document.title = `${titleElement.textContent} — MD Reader`;
  if (anchor) document.getElementById(anchor)?.scrollIntoView();
  else readerScroll.scrollTop = 0;
}

function scheduleMermaidRendering(): void {
  mermaidObserver?.disconnect();
  const blocks = Array.from(readerView.querySelectorAll<HTMLElement>("pre > code.language-mermaid"));
  for (const block of blocks) {
    const frame = document.createElement("div");
    frame.className = "mermaid-frame";
    frame.dataset.mermaidSource = block.textContent ?? "";
    frame.innerHTML = '<div class="mermaid-loading">图表准备中…</div>';
    block.parentElement?.replaceWith(frame);
  }

  mermaidObserver = new IntersectionObserver((entries, observer) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      observer.unobserve(entry.target);
      renderMermaidDiagram(entry.target as HTMLElement);
    }
  }, { root: readerScroll, rootMargin: "700px 0px" });
  readerView.querySelectorAll<HTMLElement>(".mermaid-frame").forEach((frame) => mermaidObserver?.observe(frame));
}

function renderMermaidDiagram(frame: HTMLElement, force = false): void {
  if (!force && frame.dataset.mermaidState === "rendered") return;
  const source = frame.dataset.mermaidSource ?? "";
  frame.dataset.mermaidState = "queued";
  mermaidRenderQueue = mermaidRenderQueue.catch(() => undefined).then(async () => {
    if (!frame.isConnected) return;
    frame.dataset.mermaidState = "rendering";
    const { default: mermaid } = await import("mermaid");
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: appliedTheme === "dark" ? "dark" : "neutral",
      suppressErrorRendering: true,
      maxTextSize: 50_000,
      maxEdges: 500,
      fontFamily: 'Inter, "Segoe UI", sans-serif',
    });
    const diagram = document.createElement("div");
    diagram.className = "mermaid";
    diagram.textContent = source;
    frame.replaceChildren(diagram);
    try {
      await mermaid.run({ nodes: [diagram], suppressErrors: true });
      frame.dataset.mermaidState = "rendered";
    } catch (error) {
      frame.dataset.mermaidState = "error";
      frame.innerHTML = `<div class="mermaid-error"><strong>Mermaid 图表无法渲染</strong><span>${escapeHtml(error instanceof Error ? error.message : String(error))}</span></div>`;
    }
  });
}

function rerenderMermaidDiagrams(): void {
  readerView.querySelectorAll<HTMLElement>(".mermaid-frame").forEach((frame) => {
    if (frame.dataset.mermaidState === "rendered" || frame.dataset.mermaidState === "error") {
      renderMermaidDiagram(frame, true);
    }
  });
}

function scheduleCodeHighlighting(): void {
  codeObserver?.disconnect();
  const codeBlocks = readerView.querySelectorAll<HTMLElement>("pre code[class*='language-']");
  codeObserver = new IntersectionObserver((entries, observer) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      observer.unobserve(entry.target);
      void highlightCodeBlock(entry.target as HTMLElement);
    }
  }, { root: readerScroll, rootMargin: "500px 0px" });
  codeBlocks.forEach((block) => codeObserver?.observe(block));
}

async function highlightCodeBlock(block: HTMLElement): Promise<void> {
  const language = Array.from(block.classList)
    .find((name) => name.startsWith("language-"))
    ?.slice("language-".length);
  if (!language) return;
  const { default: highlighter } = await import("highlight.js/lib/common");
  if (block.isConnected && highlighter.getLanguage(language)) highlighter.highlightElement(block);
}

function assignHeadingIds(): Heading[] {
  const seen = new Map<string, number>();
  return Array.from(readerView.querySelectorAll<HTMLElement>("h1, h2, h3, h4, h5, h6")).map((heading, index) => {
    const base = heading.textContent?.trim().toLowerCase().replace(/[^\p{Letter}\p{Number}\s-]/gu, "").replace(/\s+/g, "-") || `section-${index + 1}`;
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    heading.id = count === 0 ? base : `${base}-${count}`;
    return { id: heading.id, text: heading.textContent?.trim() ?? "", level: Number(heading.tagName.slice(1)) };
  });
}

async function hydrateLocalImages(): Promise<void> {
  if (!documentState || !isTauri) return;
  const images = Array.from(readerView.querySelectorAll<HTMLImageElement>("img"));
  let cursor = 0;
  const loadNext = async (): Promise<void> => {
    while (cursor < images.length) {
      const image = images[cursor++];
      const source = image.getAttribute("src") ?? "";
      if (!source || /^(data:|https?:|blob:)/i.test(source)) continue;
      try {
        image.src = await invoke<string>("read_local_image", { baseDir: documentState?.parent, source });
      } catch {
        image.title = `无法载入本地图片：${source}`;
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(4, images.length) }, loadNext));
}

function renderSidebar(): void {
  document.querySelectorAll<HTMLElement>("[data-tab]").forEach((button) => {
    button.classList.toggle("active", button.dataset.tab === sidebarTab);
  });
  if (sidebarTab === "outline") {
    const headings = documentState ? assignHeadingIds() : [];
    sidebarContent.innerHTML = headings.length
      ? `<div class="section-label"><span>文档大纲</span></div>${headings.map((heading) => `<button class="nav-item outline-item ${heading.level === 2 ? "level-2" : heading.level >= 3 ? (heading.level === 3 ? "level-3" : "level-deep") : ""}" data-heading-id="${escapeHtml(heading.id)}"><span>${escapeHtml(heading.text)}</span></button>`).join("")}`
      : `<div class="folder-label">当前文档没有标题</div>`;
    return;
  }
  const query = filterText.trim().toLocaleLowerCase();
  const visible = directoryFiles.filter((file) => !query || file.relativePath.toLocaleLowerCase().includes(query));
  if (directoryFiles.length === 0) {
    sidebarContent.innerHTML = `<div class="section-label"><span>最近打开</span></div>${recents.length
      ? recents.map((file) => fileButton(file.path, file.name)).join("")
      : `<div class="folder-label">打开文件夹后在这里浏览文档</div>`}`;
    return;
  }
  let currentFolder = "";
  sidebarContent.innerHTML = `<div class="section-label"><span>${query ? `${visible.length} 个结果` : `${directoryFiles.length} 个文档`}</span></div>` + visible.map((file) => {
    const folder = file.relativePath.includes("/") ? file.relativePath.slice(0, file.relativePath.lastIndexOf("/")) : "";
    const label = folder !== currentFolder ? `<div class="folder-label">${escapeHtml(folder || "根目录")}</div>` : "";
    currentFolder = folder;
    return label + fileButton(file.path, file.name);
  }).join("");
}

function fileButton(path: string, name: string): string {
  const active = documentState?.path === path ? " active" : "";
  return `<button class="nav-item${active}" data-file-path="${escapeHtml(path)}" title="${escapeHtml(path)}">${icon("file")}<span>${escapeHtml(name.replace(/\.(md|markdown|mdown|mkd|mdx|txt)$/i, ""))}</span></button>`;
}

function showToast(message: string): void {
  const toast = required<HTMLElement>("#toast");
  toast.textContent = message;
  toast.classList.add("visible");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toast.classList.remove("visible"), 2600);
}

function toggleSidebar(): void {
  updateSetting({ sidebarOpen: !settings.sidebarOpen });
}

async function handleDocumentLink(anchor: HTMLAnchorElement): Promise<void> {
  const href = anchor.getAttribute("href") ?? "";
  if (!href) return;
  if (href.startsWith("#")) {
    document.getElementById(decodeURIComponent(href.slice(1)))?.scrollIntoView();
    return;
  }
  if (/^https?:\/\//i.test(href)) {
    if (isTauri) await openUrl(href);
    else window.open(href, "_blank", "noopener");
    return;
  }
  if (documentState && /\.(md|markdown)(?:#.*)?$/i.test(href)) {
    const resolved = await invoke<{ path: string; anchor?: string }>("resolve_markdown_link", { baseDir: documentState.parent, href });
    await loadFile(resolved.path, resolved.anchor);
  }
}

document.addEventListener("click", (event) => {
  const target = event.target as Element;
  const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;
  if (action === "open-file") void pickFile();
  if (action === "open-folder") void pickFolder();
  if (action === "clear-recents") { clearRecents(); recents = []; renderRecents(); renderSidebar(); }

  const filePath = target.closest<HTMLElement>("[data-file-path]")?.dataset.filePath;
  const recentPath = target.closest<HTMLElement>("[data-recent-path]")?.dataset.recentPath;
  if (filePath || recentPath) void loadFile(filePath ?? recentPath ?? "");

  const headingId = target.closest<HTMLElement>("[data-heading-id]")?.dataset.headingId;
  if (headingId) document.getElementById(headingId)?.scrollIntoView();

  const tab = target.closest<HTMLElement>("[data-tab]")?.dataset.tab as SidebarTab | undefined;
  if (tab) { sidebarTab = tab; renderSidebar(); }

  const theme = target.closest<HTMLElement>("[data-theme-choice]")?.dataset.themeChoice as ThemePreference | undefined;
  if (theme) updateSetting({ theme });

  const anchor = target.closest<HTMLAnchorElement>(".markdown-body a");
  if (anchor) { event.preventDefault(); void handleDocumentLink(anchor).catch((error) => showToast(String(error))); }

  if (!target.closest("#settings-popover, #settings-toggle")) settingsPopover.hidden = true;
});

required("#open-file").addEventListener("click", () => void pickFile());
required("#open-folder").addEventListener("click", () => void pickFolder());
required("#toggle-sidebar").addEventListener("click", toggleSidebar);
required("#settings-toggle").addEventListener("click", () => { settingsPopover.hidden = !settingsPopover.hidden; });
required("#theme-toggle").addEventListener("click", () => updateSetting({ theme: document.documentElement.dataset.theme === "dark" ? "light" : "dark" }));

fileSearch.addEventListener("input", () => { filterText = fileSearch.value; sidebarTab = "files"; renderSidebar(); });
required<HTMLInputElement>("#font-size").addEventListener("input", (event) => updateSetting({ fontSize: Number((event.target as HTMLInputElement).value) }));
required<HTMLInputElement>("#content-width").addEventListener("input", (event) => updateSetting({ contentWidth: Number((event.target as HTMLInputElement).value) }));

document.addEventListener("keydown", (event) => {
  if (!(event.ctrlKey || event.metaKey)) return;
  if (event.key.toLowerCase() === "o") {
    event.preventDefault();
    if (event.shiftKey) void pickFolder(); else void pickFile();
  }
  if (event.key.toLowerCase() === "b") { event.preventDefault(); toggleSidebar(); }
  if (event.key.toLowerCase() === "k") { event.preventDefault(); if (!settings.sidebarOpen) updateSetting({ sidebarOpen: true }); fileSearch.focus(); }
});

matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { if (settings.theme === "system") applySettings(); });

async function initializeDesktopEvents(): Promise<void> {
  if (!isTauri) return;
  await getCurrentWindow().onDragDropEvent((event) => {
    if (event.payload.type === "over") dropOverlay.hidden = false;
    if (event.payload.type === "leave") dropOverlay.hidden = true;
    if (event.payload.type === "drop") {
      dropOverlay.hidden = true;
      const path = event.payload.paths.find((candidate) => /\.(md|markdown|mdown|mkd|mdx|txt)$/i.test(candidate));
      if (path) void loadFile(path); else showToast("请拖入 Markdown 文件");
    }
  });
  const startupPath = await invoke<string | null>("startup_markdown_file");
  if (startupPath) await loadFile(startupPath);
  window.setInterval(async () => {
    if (!documentState || document.hidden) return;
    try {
      const modifiedMs = await invoke<number>("file_modified_ms", { path: documentState.path });
      if (modifiedMs > documentState.modifiedMs) await loadFile(documentState.path);
    } catch {
      // The next explicit open will surface file access errors.
    }
  }, 2_000);
}

applySettings();
showWelcome();
renderSidebar();
void initializeDesktopEvents().catch((error) => showToast(`初始化失败：${String(error)}`));
