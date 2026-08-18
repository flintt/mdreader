import { performance } from "node:perf_hooks";
import hljs from "highlight.js/lib/common";
import { Marked } from "marked";

const parser = new Marked({ gfm: true, breaks: false });

const block = `
## 快速渲染

Markdown 阅读器需要在文字、代码与表格之间保持稳定排版。这里包含 **强调**、[链接](https://example.com) 和任务列表。

- [x] 后台解析
- [x] 代码高亮
- [ ] 继续优化

| 项目 | 状态 | 延迟 |
| --- | --- | ---: |
| parser | ready | 4 ms |
| layout | ready | 8 ms |

\`\`\`typescript
export function render(source: string): string {
  return source.trim();
}
\`\`\`
`;

function sourceOfApproximateSize(bytes) {
  const repeats = Math.ceil(bytes / Buffer.byteLength(block));
  return `# MD Reader benchmark\n${block.repeat(repeats)}`;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

parser.parse(block);
console.log("Markdown parse (Node.js, median of 7 warm runs)");
for (const requestedBytes of [100_000, 1_000_000]) {
  const source = sourceOfApproximateSize(requestedBytes);
  const samples = [];
  for (let run = 0; run < 7; run += 1) {
    const startedAt = performance.now();
    parser.parse(source);
    samples.push(performance.now() - startedAt);
  }
  console.log(`${(Buffer.byteLength(source) / 1_000).toFixed(0)} KB\t${median(samples).toFixed(1)} ms`);
}

const highlightSamples = [];
const code = "export function render(source: string): string { return source.trim(); }";
for (let run = 0; run < 7; run += 1) {
  const startedAt = performance.now();
  for (let visibleBlock = 0; visibleBlock < 20; visibleBlock += 1) {
    hljs.highlight(code, { language: "typescript" });
  }
  highlightSamples.push(performance.now() - startedAt);
}
console.log(`20 visible code blocks\t${median(highlightSamples).toFixed(1)} ms`);
